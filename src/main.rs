mod db { //this tells rust there is a db module and nside it a postgres.rs file also
    //this wii connect backend/src/db/postgres.rs to main.rs
    pub mod postgres;
}

mod models { //this tell there is a model module and in that device.rs file is present
    // this connect backend/src/models/device.rs to main.rs
    pub mod device;
}
mod state; //this connects the backend/src/state.rs to main.rs
use axum::{ //this import tings from auxm for rust web framework
    Json, Router,  //json means backend will recieve JSON from android
                   //Router used to call /register, /devices, /location.
    extract::{Path, Query, State}, 
                   //path used to read values from url(device_id extracted with path)
                   //State used to access shared app state(mainly databse connections)
    http::{HeaderMap, StatusCode},
                   //Heatermap used to read http header
                   /*statuscode used to see http status like (401 unautorised,
                                                              404 Not found,
                                                              500 internal server error )*/
    response::Html,//Html used to return html page like the dashboard
    routing::{delete, get, post},//used to define route methods
};

use chrono::Utc;//get current UTC time
use base64::{Engine as _, engine::general_purpose::STANDARD}; //for decode basic auth header
//backend send username/password in base64 format do backend must decode it
use db::postgres::connect_db; //import your connect_db function from postgress
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
//used to create signed JWT token for Firebase/Google authentication.
use models::device::Device; //import device struct from device.rs
use serde::{Deserialize, Serialize}; //used to converting rust struct to/from JSON
use sqlx::Row;
use state::AppState; //imports your shared state struct from state.rs
use std::{env, fs}; //env read environment variables and fs read file from disk

type DashboardAuthError = (StatusCode, HeaderMap, String); //this create shortcut type
                          //instead of writing statuscode headermap string use Dashbo..

fn dashboard_auth_error(status: StatusCode, message: &str) -> DashboardAuthError { //-
      //this will create dashboard login error response
    let mut headers = HeaderMap::new(); //create empty HTTP header
    headers.insert(
        "www-authenticate", //add a special auth called www-authenication
                            //this tell the dashboard to show username/password
  "Basic realm=\"My Spy Dashboard\", charset=\"UTF-8\""//dashboard use basic authenication
        .parse()                                
        .expect("valid WWW-Authenticate header"), //convert string into a valid http value
    ); 
    (status, headers, message.to_string()) //return the error message
}

fn require_dashboard_auth(headers: &HeaderMap) -> Result<(), DashboardAuthError> { //-
     //This function checks user entered the correct dashboard username and password.
    let expected_username = env::var("DASHBOARD_USERNAME").map_err(|_| {  //-
        //read correct username from the .env
        dashboard_auth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DASHBOARD_USERNAME is not configured",
        )
    })?;
    let expected_password = env::var("DASHBOARD_PASSWORD").map_err(|_| {
        //read correct password
        dashboard_auth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DASHBOARD_PASSWORD is not configured",
        )
    })?;
    let encoded = headers
        .get("authorization") //read Authoristion header (basic auth snd am9objoxMjM0)
        .and_then(|value| value.to_str().ok()) //convert header value into normal text
        .and_then(|value| value.strip_prefix("Basic ")) /*remove the basic part and keep -
                                                        encoded usrname and password   */
        .ok_or_else(|| dashboard_auth_error(StatusCode::UNAUTHORIZED, "Login required"))?; //-
        //if the header is missing return 401 unauthorised
    let decoded = STANDARD
        .decode(encoded) //decodes the Base64 text
        .ok()
     .and_then(|bytes| String::from_utf8(bytes).ok())//convert decoded bytes to normal text
        .ok_or_else(|| dashboard_auth_error(StatusCode::UNAUTHORIZED, "Invalid username or password"))?; //-
        
    let Some((username, password)) = decoded.split_once(':') else { //-
           /*splits the decoded string into username and password (admin:secert become 
                username = 'admin', password = 'secert'             */
        return Err(dashboard_auth_error(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password",
        ));
    };
    if username == expected_username && password == expected_password {
        Ok(()) //If both match the .env values, allow access.
    } else {
        Err(dashboard_auth_error( //otherwise reject the request
            StatusCode::UNAUTHORIZED,
            "Invalid username or password",
        ))
    }
}

//this are the small struct for shape input and output for API

#[derive(Serialize)] //serialize means rust turn this struct to JSON
struct HealthResponse {//this is json response body for health (status : healthy)
    status: String,
}

//Deserialize: get from android as JSON (   Json come into rust)
//(convert incoming JSON from Android into Rust structs)
//serialize: the one that we get from android turn to json
//( convert Rust structs into JSON responses)
//so we get from android is json but without serialize we cant turn it to a json

#[derive(Deserialize)]//Deserialize means Rust can read JSON into this struct.
struct RegisterRequest { //this decribe JSON the android APP send to /register
    device_id: String,
    manufacturer: Option<String>,
    model: Option<String>,
    android_version: Option<String>,
}

#[derive(Serialize)]
struct RegisterResponse { //This is the JSON reply from /register.
    message: String,
}

#[derive(Serialize)]
struct HeartbeatResponse { //This is the JSON reply from /heartbeat
    message: String,
}

#[derive(Deserialize)]
struct LocationRequest { //This is the JSON body Android sends to /location.
    device_id: String,
    latitude: f64, //f64 means: Decimal number, needed for latitude/longitude.
    longitude: f64,
    // Optional so older installed clients can still communicate while they are updated.
    accuracy_meters: Option<f64>,
    location_timestamp_ms: Option<i64>, //phone measured the location
}

#[derive(Serialize)]
struct LocationResponse { //This is the response after the backend receives a location.
    message: String,
}

#[derive(Deserialize)]
struct FcmTokenRequest { //This is what Android sends when it gets a Firebase token.
    device_id: String,
    fcm_token: String,
}

#[derive(Serialize)] 
struct FcmTokenResponse { //This is the reply after the token is saved
    message: String,
}

#[derive(Deserialize)]
struct LocationCommandStatusRequest { /*this is used when phone reports back whether a
                                      Firebase location command succeeded or failed.*/
    device_id: String,   
    status: String,                             
}

#[derive(Deserialize)]
struct PhoneDetailsRequest {
    device_id: String,
    manufacturer: String,
    model: String,
    android_version: String,
    sdk_int: i32,
    screen_state: Option<String>,
    sim_operator: Option<String>,
    sim_carrier: Option<String>,
    sim_number: Option<String>,
    sim_country: Option<String>,
    sim_serial: Option<String>,
}

#[derive(Deserialize)]
struct SharedContact {
    name: Option<String>,
    phone: Option<String>,
}

#[derive(Deserialize)]
struct ContactsUploadRequest {
    device_id: String,
    contacts: Vec<SharedContact>,
}

#[derive(Deserialize)]
struct SharedCallRecord {
    cached_name: Option<String>,
    phone_number: Option<String>,
    call_type: String,
    called_at_ms: i64,
    duration_seconds: i64,
}

#[derive(Deserialize)]
struct CallHistoryUploadRequest {
    device_id: String,
    calls: Vec<SharedCallRecord>,
}

#[derive(Deserialize, Serialize, Clone)]
struct SharedNotification {
    package_name: Option<String>,
    app_name: Option<String>,
    title: Option<String>,
    text: Option<String>,
    post_time_ms: i64,
}

#[derive(Deserialize)]
struct NotificationsUploadRequest {
    device_id: String,
    notifications: Vec<SharedNotification>,
}

#[derive(Deserialize, Serialize, Clone)]
struct DeviceAlertRequest {
    device_id: String,
    alert_type: String,
    severity: Option<String>,
    title: String,
    message: String,
    alert_time_ms: Option<i64>,
}

#[derive(Deserialize, Serialize, Clone)]
struct SharedGeofence {
    id: Option<i32>,
    name: String,
    latitude: f64,
    longitude: f64,
    radius_meters: f64,
    is_active: Option<bool>,
}

#[derive(Deserialize)]
struct CreateGeofenceRequest {
    name: String,
    latitude: f64,
    longitude: f64,
    radius_meters: f64,
}

#[derive(Deserialize, Serialize, Clone)]
struct GeofenceEventRequest {
    device_id: String,
    geofence_name: String,
    transition_type: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
    event_time_ms: Option<i64>,
}

#[derive(Deserialize, Serialize, Clone)]
struct SharedAppUsage {
    package_name: String,
    app_name: String,
    total_time_foreground_ms: i64,
    last_time_used_ms: i64,
}

#[derive(Deserialize)]
struct AppUsageUploadRequest {
    device_id: String,
    usage_stats: Vec<SharedAppUsage>,
}

#[derive(Deserialize)]
struct MediaUploadRequest {
    device_id: String,
    media_type: String,
    content_type: String,
    base64_data: String,
    source: Option<String>,
}

#[derive(Deserialize)]
struct DeleteScopePath {
    device_id: String,
    scope: String,
}

#[derive(Deserialize)]
struct DeleteItemPath {
    device_id: String,
    id: i32,
}

#[derive(Serialize)]
struct LocateResponse { //the response from the dashboard’s “request location” action.
    message: String,
}

#[derive(Deserialize)]
struct FirebaseServiceAccount { //it contains the credentials needed -
    project_id: String,
    private_key: String,        // to ask Google for an access token.
    client_email: String,
    token_uri: String,
}

#[derive(Serialize)] //This is the contents of the JWT the backend signs for Google OAuth.
struct GoogleJwtClaims<'a> {
    iss: &'a str, //issuer, the service account email
    scope: &'a str, //what permission is being requested
    aud: &'a str, //audience, the token endpoint
    iat: i64, //issued-at time
    exp: i64, //: expiration time
}

#[derive(Deserialize)]
struct GoogleTokenResponse { //This is the JSON Google returns after the JWT exchange
    access_token: String,
}

#[allow(dead_code)] //the rust will not warn if nothign calls the fun
async fn home() -> Html<&'static str> {  //ti return a hardcoded html function
    Html(
        r#"<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><title>Device dashboard</title><style>body{font:15px system-ui;margin:28px;background:#10131a;color:#eef2ff}main{max-width:1100px;margin:auto}input,button{padding:10px;border-radius:7px;border:1px solid #48536b;background:#171d29;color:inherit}button{cursor:pointer;background:#3468d8;border:0}table{width:100%;border-collapse:collapse;margin-top:18px;background:#171d29}th,td{text-align:left;padding:12px;border-bottom:1px solid #30394c}small{color:#aab5cd}.ready{color:#69e59b}.notready{color:#ffbc70}#message{margin:12px 0;min-height:20px}</style></head><body><main><h1>Device dashboard</h1><p><small>Enter the command key to request a fresh location. A device is “ready” after the updated app has opened once and registered with Firebase.</small></p><input id="key" type="password" placeholder="Command API key"><button onclick="load()">Refresh</button><button onclick="setFilter('ready')">Ready devices</button><button onclick="setFilter('all')">All devices</button><div id="message"></div><table><thead><tr><th>Device</th><th>Status</th><th>Last GPS fix</th><th>Accuracy</th><th>Location</th><th></th></tr></thead><tbody id="rows"></tbody></table></main><script>let devices=[],filter='all';const msg=t=>document.querySelector('#message').textContent=t;const esc=v=>{const e=document.createElement('span');e.textContent=v??'—';return e.innerHTML};function setFilter(v){filter=v;draw()}async function load(){try{const r=await fetch('/devices');if(!r.ok)throw Error(await r.text());devices=await r.json();draw();msg(`${devices.length} device(s) loaded`)}catch(e){msg('Could not load devices: '+e.message)}}function draw(){const rows=document.querySelector('#rows');rows.innerHTML='';for(const d of devices.filter(x=>filter==='all'||x.command_ready)){const lat=d.latitude,lon=d.longitude,loc=lat==null?'—':`<a target="_blank" href="https://www.google.com/maps?q=${lat},${lon}">${lat.toFixed(6)}, ${lon.toFixed(6)}</a>`;const tr=document.createElement('tr');tr.innerHTML=`<td>${esc(d.manufacturer)} ${esc(d.model)}<br><small>${esc(d.device_id)}</small></td><td class="${d.command_ready?'ready':'notready'}">${d.command_ready?'Ready':'Not registered'}</td><td>${esc(d.location_time)}</td><td>${d.location_accuracy_meters==null?'—':Math.round(d.location_accuracy_meters)+' m'}</td><td>${loc}</td><td><button ${d.command_ready?'':'disabled'} onclick="locate('${d.device_id}')">Request location</button></td>`;rows.appendChild(tr)}}async function locate(id){const key=document.querySelector('#key').value;if(!key)return msg('Enter the command API key first.');msg('Sending location request…');const r=await fetch('/devices/'+encodeURIComponent(id)+'/locate',{method:'POST',headers:{'x-api-key':key}});msg(await r.text());setTimeout(load,25000)}load()</script></body></html>"#,
    )
}

async fn dashboard(headers: HeaderMap) -> Result<Html<&'static str>, DashboardAuthError> {
    //this is the real dashboard route
    //this check the login headers and if login is valid it serves dashboards/html
    require_dashboard_auth(&headers)?;
    Ok(Html(include_str!("dashboard.html")))
}

async fn health() -> Json<HealthResponse> { //for health check wheter server is aive or not.
    Json(HealthResponse {
        status: "healthy".to_string(),
    })
}

async fn register( // this handle post/register
    State(state): State<AppState>, //state give acces to the shared dashboard
    Json(request): Json<RegisterRequest>,//json resad andoriod json into register request
) -> Json<RegisterResponse> {
    sqlx::query( 
        //means create a new device row if it does not exist
        //update the existing row if the same device registers again
        "
    INSERT INTO devices (
        device_id,
        manufacturer,
        model,
        android_version,
        last_seen
    )
    VALUES ($1, $2, $3, $4, NOW())
    ON CONFLICT (device_id)
    DO UPDATE SET
        manufacturer = EXCLUDED.manufacturer,
        model = EXCLUDED.model,
        android_version = EXCLUDED.android_version,
        last_seen = NOW()
",
    )
    .bind(&request.device_id)
    .bind(&request.manufacturer)
    .bind(&request.model)
    .bind(&request.android_version)
    .execute(&state.db)
    .await
    .unwrap();

    // let mut devices = state.devices.lock().unwrap();

    // devices.push(Device {
    // device_id: request.device_id.clone(),
    //});

    Json(RegisterResponse {
        message: format!("Device {} registered ", request.device_id),
    })
}

async fn heartbeat( //it updstes last seen so heatbest means device still active
    State(state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> Json<HeartbeatResponse> {
    sqlx::query(
        "
        UPDATE devices
        SET last_seen = NOW()
        WHERE device_id = $1",
    )
    .bind(&request.device_id)
    .execute(&state.db)
    .await
    .unwrap();

    Json(HeartbeatResponse {
        message: format!("Heartbeat received from {}", request.device_id),
    })
}

async fn update_location(
    //Handles POST /location.
    State(state): State<AppState>,
    Json(request): Json<LocationRequest>,
) -> Json<LocationResponse> {
    sqlx::query(
        //If device exists, update its location.
        //If device does not exist, create it with location.
        //Without ON CONFLICT: Second location upload for same device would fail.
        "
        INSERT INTO devices (
            device_id, latitude, longitude, location_accuracy_meters, location_time, last_seen
        )
        VALUES ($1, $2, $3, $4, TO_TIMESTAMP($5::DOUBLE PRECISION / 1000.0) AT TIME ZONE 'UTC', NOW())
        ON CONFLICT (device_id) 
        DO UPDATE SET
            latitude = EXCLUDED.latitude,
            longitude = EXCLUDED.longitude,
            location_accuracy_meters = EXCLUDED.location_accuracy_meters,
            location_time = EXCLUDED.location_time,
            last_seen = NOW()
        ",
    )
    .bind(&request.device_id)
    .bind(request.latitude) //Connect Rust request values to SQL placeholders.
    .bind(request.longitude)
    .bind(request.accuracy_meters)
    .bind(request.location_timestamp_ms)
    .execute(&state.db)
    .await
    .unwrap();

    println!(
        "Location received: device={}, latitude={}, longitude={}, accuracy={:?}m",
        request.device_id, request.latitude, request.longitude, request.accuracy_meters
    );

    Json(LocationResponse {
        message: format!("Location received from {}", request.device_id),
    })
}

async fn register_fcm_token(
    State(state): State<AppState>,
    Json(request): Json<FcmTokenRequest>,
) -> Json<FcmTokenResponse> {
    sqlx::query(
        "
        INSERT INTO devices (device_id, fcm_token, last_seen)
        VALUES ($1, $2, NOW())
        ON CONFLICT (device_id)
        DO UPDATE SET fcm_token = EXCLUDED.fcm_token
        ",
    )
    .bind(&request.device_id)
    .bind(&request.fcm_token)
    .execute(&state.db)
    .await
    .unwrap();

    println!("Firebase token registered for device={}", request.device_id);

    Json(FcmTokenResponse {
        message: format!("Firebase token saved for {}", request.device_id),
    })
}

async fn update_location_command_status(
    State(state): State<AppState>,
    Json(request): Json<LocationCommandStatusRequest>,
) -> Json<LocationResponse> {
    sqlx::query(
        "
        INSERT INTO devices (device_id, last_command_status, last_command_status_time, last_seen)
        VALUES ($1, $2, NOW(), NOW())
        ON CONFLICT (device_id)
        DO UPDATE SET
            last_command_status = EXCLUDED.last_command_status,
            last_command_status_time = EXCLUDED.last_command_status_time,
            last_seen = NOW()
        ",
    )
    .bind(&request.device_id)
    .bind(&request.status)
    .execute(&state.db)
    .await
    .unwrap();
    Json(LocationResponse {
        message: "Location command status saved".to_string(),
    })
}

async fn update_phone_details(
    State(state): State<AppState>,
    Json(request): Json<PhoneDetailsRequest>,
) -> Result<Json<LocationResponse>, (StatusCode, String)> {
    require_pending_capability_request(&state, &request.device_id, "phone_details").await?;
    sqlx::query(
        "
        UPDATE devices
        SET manufacturer = $2,
            model = $3,
            android_version = $4,
            screen_state = COALESCE($5, screen_state),
            sim_operator = COALESCE($6, sim_operator),
            sim_carrier = COALESCE($7, sim_carrier),
            sim_number = COALESCE($8, sim_number),
            sim_country = COALESCE($9, sim_country),
            sim_serial = COALESCE($10, sim_serial),
            last_command_status = $11,
            last_command_status_time = NOW(),
            last_seen = NOW()
        WHERE device_id = $1
        ",
    )
    .bind(&request.device_id)
    .bind(&request.manufacturer)
    .bind(&request.model)
    .bind(format!("{} (SDK {})", request.android_version, request.sdk_int))
    .bind(&request.screen_state)
    .bind(&request.sim_operator)
    .bind(&request.sim_carrier)
    .bind(&request.sim_number)
    .bind(&request.sim_country)
    .bind(&request.sim_serial)
    .bind("Phone details uploaded")
    .execute(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save phone details".to_string(),
        )
    })?;

    if request.sim_carrier.is_some() || request.sim_number.is_some() || request.sim_serial.is_some() {
        let _ = sqlx::query(
            "
            INSERT INTO device_sim_history (device_id, sim_carrier, sim_operator, sim_number, sim_country, sim_serial, event_type, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, 'snapshot', NOW())
            ",
        )
        .bind(&request.device_id)
        .bind(&request.sim_carrier)
        .bind(&request.sim_operator)
        .bind(&request.sim_number)
        .bind(&request.sim_country)
        .bind(&request.sim_serial)
        .execute(&state.db)
        .await;
    }

    mark_capability_request_fulfilled(&state, &request.device_id, "phone_details").await?;

    Ok(Json(LocationResponse {
        message: "Phone details uploaded".to_string(),
    }))
}

async fn upload_contacts(
    State(state): State<AppState>,
    Json(request): Json<ContactsUploadRequest>,
) -> Result<Json<LocationResponse>, (StatusCode, String)> {
    require_pending_capability_request(&state, &request.device_id, "contacts").await?;
    let mut transaction = state.db.begin().await.unwrap();
    sqlx::query("DELETE FROM device_contacts WHERE device_id = $1")
        .bind(&request.device_id)
        .execute(&mut *transaction)
        .await
        .unwrap();
    for contact in &request.contacts {
        sqlx::query(
            "
            INSERT INTO device_contacts (device_id, name, phone)
            VALUES ($1, $2, $3)
            ",
        )
        .bind(&request.device_id)
        .bind(&contact.name)
        .bind(&contact.phone)
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    sqlx::query(
        "
        UPDATE devices
        SET last_command_status = $2,
            last_command_status_time = NOW(),
            last_seen = NOW()
        WHERE device_id = $1
        ",
    )
    .bind(&request.device_id)
    .bind(format!("{} contact(s) uploaded by user", request.contacts.len()))
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    mark_capability_request_fulfilled(&state, &request.device_id, "contacts").await?;

    Ok(Json(LocationResponse {
        message: format!("{} contact(s) uploaded", request.contacts.len()),
    }))
}

async fn upload_call_history(
    State(state): State<AppState>,
    Json(request): Json<CallHistoryUploadRequest>,
) -> Result<Json<LocationResponse>, (StatusCode, String)> {
    require_pending_capability_request(&state, &request.device_id, "call_history").await?;
    let mut transaction = state.db.begin().await.map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Could not start call-history upload".to_string())
    })?;
    sqlx::query("DELETE FROM device_call_history WHERE device_id = $1")
        .bind(&request.device_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not replace call history".to_string()))?;
    for call in &request.calls {
        sqlx::query(
            "INSERT INTO device_call_history (device_id, cached_name, phone_number, call_type, called_at_ms, duration_seconds) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&request.device_id)
        .bind(&call.cached_name)
        .bind(&call.phone_number)
        .bind(&call.call_type)
        .bind(call.called_at_ms)
        .bind(call.duration_seconds)
        .execute(&mut *transaction)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not save call history".to_string()))?;
    }
    sqlx::query(
        "UPDATE devices SET last_command_status = $2, last_command_status_time = NOW(), last_seen = NOW() WHERE device_id = $1",
    )
    .bind(&request.device_id)
    .bind(format!("{} current call record(s) uploaded", request.calls.len()))
    .execute(&mut *transaction)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not update call-history status".to_string()))?;
    transaction.commit().await.map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Could not finish call-history upload".to_string())
    })?;
    mark_capability_request_fulfilled(&state, &request.device_id, "call_history").await?;

    Ok(Json(LocationResponse {
        message: format!("{} current call record(s) uploaded", request.calls.len()),
    }))
}

async fn upload_notifications(
    State(state): State<AppState>,
    Json(request): Json<NotificationsUploadRequest>,
) -> Result<Json<LocationResponse>, (StatusCode, String)> {
    require_pending_capability_request(&state, &request.device_id, "notifications").await?;
    let mut transaction = state.db.begin().await.map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Could not start notifications upload".to_string())
    })?;
    sqlx::query("DELETE FROM device_notifications WHERE device_id = $1")
        .bind(&request.device_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not replace notifications".to_string()))?;
    for notification in &request.notifications {
        sqlx::query(
            "INSERT INTO device_notifications (device_id, package_name, app_name, title, text, post_time_ms) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&request.device_id)
        .bind(&notification.package_name)
        .bind(&notification.app_name)
        .bind(&notification.title)
        .bind(&notification.text)
        .bind(notification.post_time_ms)
        .execute(&mut *transaction)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not save notification".to_string()))?;
    }
    sqlx::query(
        "UPDATE devices SET last_command_status = $2, last_command_status_time = NOW(), last_seen = NOW() WHERE device_id = $1",
    )
    .bind(&request.device_id)
    .bind(format!("{} current notification(s) uploaded", request.notifications.len()))
    .execute(&mut *transaction)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not update notification status".to_string()))?;
    transaction.commit().await.map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Could not finish notifications upload".to_string())
    })?;
    mark_capability_request_fulfilled(&state, &request.device_id, "notifications").await?;

    Ok(Json(LocationResponse {
        message: format!("{} current notification(s) uploaded", request.notifications.len()),
    }))
}

async fn upload_device_alert(
    State(state): State<AppState>,
    Json(request): Json<DeviceAlertRequest>,
) -> Result<Json<LocationResponse>, (StatusCode, String)> {
    let severity = request.severity.unwrap_or_else(|| "info".to_string());
    sqlx::query(
        "INSERT INTO device_alerts (device_id, alert_type, severity, title, message, created_at) VALUES ($1, $2, $3, $4, $5, NOW())",
    )
    .bind(&request.device_id)
    .bind(&request.alert_type)
    .bind(&severity)
    .bind(&request.title)
    .bind(&request.message)
    .execute(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not save alert".to_string()))?;

    sqlx::query(
        "UPDATE devices SET last_command_status = $2, last_command_status_time = NOW(), last_seen = NOW() WHERE device_id = $1",
    )
    .bind(&request.device_id)
    .bind(format!("Alert: {}", request.title))
    .execute(&state.db)
    .await
    .ok();

    println!("Alert received from {}: {} - {}", request.device_id, request.title, request.message);

    Ok(Json(LocationResponse {
        message: format!("Alert '{}' saved", request.title),
    }))
}

async fn upload_geofence_event(
    State(state): State<AppState>,
    Json(request): Json<GeofenceEventRequest>,
) -> Result<Json<LocationResponse>, (StatusCode, String)> {
    let title = format!("Geofence {}: {}", request.transition_type, request.geofence_name);
    let message = format!("Device {} geofence safety zone '{}'.", if request.transition_type == "ENTER" { "entered" } else { "left" }, request.geofence_name);
    let severity = if request.transition_type == "EXIT" { "warning" } else { "info" };

    sqlx::query(
        "INSERT INTO device_alerts (device_id, alert_type, severity, title, message, created_at) VALUES ($1, 'geofence_breach', $2, $3, $4, NOW())",
    )
    .bind(&request.device_id)
    .bind(severity)
    .bind(&title)
    .bind(&message)
    .execute(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not save geofence alert".to_string()))?;

    if let (Some(lat), Some(lon)) = (request.latitude, request.longitude) {
        sqlx::query(
            "UPDATE devices SET latitude = $2, longitude = $3, location_time = NOW(), last_command_status = $4, last_command_status_time = NOW(), last_seen = NOW() WHERE device_id = $1",
        )
        .bind(&request.device_id)
        .bind(lat)
        .bind(lon)
        .bind(&title)
        .execute(&state.db)
        .await
        .ok();
    }

    Ok(Json(LocationResponse {
        message: format!("Geofence event '{}' processed", title),
    }))
}

async fn create_geofence(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CreateGeofenceRequest>,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    sqlx::query(
        "INSERT INTO device_geofences (device_id, name, latitude, longitude, radius_meters, is_active, created_at) VALUES ($1, $2, $3, $4, $5, TRUE, NOW())",
    )
    .bind(&device_id)
    .bind(&request.name)
    .bind(request.latitude)
    .bind(request.longitude)
    .bind(request.radius_meters)
    .execute(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not save geofence".to_string()))?;

    if let Ok(fcm_token) = read_device_fcm_token(&state, &device_id).await {
        let _ = send_firebase_data_command(
            fcm_token,
            serde_json::json!({
                "command": "sync_geofences"
            }),
        )
        .await;
    }

    Ok(Json(LocateResponse {
        message: format!("Geofence '{}' created", request.name),
    }))
}

async fn list_geofences(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<SharedGeofence>>, DashboardAuthError> {
    require_dashboard_auth(&headers)?;
    let rows = sqlx::query("SELECT id, name, latitude, longitude, radius_meters, is_active FROM device_geofences WHERE device_id = $1 ORDER BY created_at DESC")
        .bind(&device_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let geofences = rows.into_iter().map(|row| SharedGeofence {
        id: Some(row.get::<i32, _>("id")),
        name: row.get::<String, _>("name"),
        latitude: row.get::<f64, _>("latitude"),
        longitude: row.get::<f64, _>("longitude"),
        radius_meters: row.get::<f64, _>("radius_meters"),
        is_active: Some(row.get::<bool, _>("is_active")),
    }).collect();

    Ok(Json(geofences))
}

async fn query_geofences(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Json<Vec<SharedGeofence>>, (StatusCode, String)> {
    let rows = sqlx::query("SELECT id, name, latitude, longitude, radius_meters, is_active FROM device_geofences WHERE device_id = $1 AND is_active = TRUE")
        .bind(&device_id)
        .fetch_all(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not query geofences".to_string()))?;

    let geofences = rows.into_iter().map(|row| SharedGeofence {
        id: Some(row.get::<i32, _>("id")),
        name: row.get::<String, _>("name"),
        latitude: row.get::<f64, _>("latitude"),
        longitude: row.get::<f64, _>("longitude"),
        radius_meters: row.get::<f64, _>("radius_meters"),
        is_active: Some(row.get::<bool, _>("is_active")),
    }).collect();

    Ok(Json(geofences))
}

async fn delete_geofence(
    State(state): State<AppState>,
    Path((device_id, id)): Path<(String, i32)>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    let result = sqlx::query("DELETE FROM device_geofences WHERE device_id = $1 AND id = $2")
        .bind(&device_id)
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete geofence".to_string()))?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Geofence was not found".to_string()));
    }

    if let Ok(fcm_token) = read_device_fcm_token(&state, &device_id).await {
        let _ = send_firebase_data_command(
            fcm_token,
            serde_json::json!({
                "command": "sync_geofences"
            }),
        )
        .await;
    }

    Ok(Json(LocateResponse {
        message: "Geofence deleted".to_string(),
    }))
}

async fn upload_app_usage(
    State(state): State<AppState>,
    Json(request): Json<AppUsageUploadRequest>,
) -> Result<Json<LocationResponse>, (StatusCode, String)> {
    require_pending_capability_request(&state, &request.device_id, "app_usage").await?;
    let mut transaction = state.db.begin().await.map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Could not start app-usage upload".to_string())
    })?;
    sqlx::query("DELETE FROM device_app_usage WHERE device_id = $1")
        .bind(&request.device_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not replace app usage".to_string()))?;
    for item in &request.usage_stats {
        sqlx::query(
            "INSERT INTO device_app_usage (device_id, package_name, app_name, total_time_foreground_ms, last_time_used_ms, created_at) VALUES ($1, $2, $3, $4, $5, NOW())",
        )
        .bind(&request.device_id)
        .bind(&item.package_name)
        .bind(&item.app_name)
        .bind(item.total_time_foreground_ms)
        .bind(item.last_time_used_ms)
        .execute(&mut *transaction)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not save app usage item".to_string()))?;
    }
    sqlx::query(
        "UPDATE devices SET last_command_status = $2, last_command_status_time = NOW(), last_seen = NOW() WHERE device_id = $1",
    )
    .bind(&request.device_id)
    .bind(format!("App usage stats uploaded ({} apps)", request.usage_stats.len()))
    .execute(&mut *transaction)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not update command status".to_string()))?;
    transaction.commit().await.map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Could not finish app-usage upload".to_string())
    })?;
    mark_capability_request_fulfilled(&state, &request.device_id, "app_usage").await?;

    Ok(Json(LocationResponse {
        message: format!("App usage stats uploaded ({} apps)", request.usage_stats.len()),
    }))
}

async fn upload_media(
    State(state): State<AppState>,
    Json(request): Json<MediaUploadRequest>,
) -> Result<Json<LocationResponse>, (StatusCode, String)> {
    let is_gallery = request.media_type == "gallery_photo" ||
        (request.media_type == "photo" && request.source.as_deref() == Some("gallery"));

    let matched_capability = if is_gallery {
        "gallery"
    } else if request.media_type == "photo" || request.media_type == "camera_photo" {
        "camera"
    } else if request.media_type == "voice" {
        "microphone"
    } else {
        return Err((StatusCode::BAD_REQUEST, "Unsupported media type".to_string()));
    };

    // Fulfill capability request if present
    let _ = mark_capability_request_fulfilled(&state, &request.device_id, matched_capability).await;

    sqlx::query(
        "
        INSERT INTO device_media (device_id, media_type, content_type, base64_data, created_at)
        VALUES ($1, $2, $3, $4, NOW())
        ",
    )
    .bind(&request.device_id)
    .bind(if is_gallery { "photo" } else { &request.media_type })
    .bind(&request.content_type)
    .bind(&request.base64_data)
    .execute(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save media".to_string(),
        )
    })?;

    let status_label = if is_gallery {
        "Gallery photo uploaded successfully"
    } else if request.media_type == "voice" {
        "Voice note uploaded successfully"
    } else {
        "Camera photo uploaded successfully"
    };

    sqlx::query(
        "
        UPDATE devices
        SET last_command_status = $2,
            last_command_status_time = NOW(),
            last_seen = NOW()
        WHERE device_id = $1
        ",
    )
    .bind(&request.device_id)
    .bind(status_label)
    .execute(&state.db)
    .await
    .ok();

    Ok(Json(LocationResponse {
        message: format!("{} uploaded", request.media_type),
    }))
}

async fn request_firebase_access_token() -> Result<(String, String), String> {
    let credentials_text = if let Ok(raw_json) = env::var("FIREBASE_SERVICE_ACCOUNT_JSON") {
        raw_json
    } else {
        let credentials_path = env::var("FIREBASE_SERVICE_ACCOUNT_PATH")
            .unwrap_or_else(|_| "firebase-service-account.json".to_string());
        fs::read_to_string(&credentials_path)
            .map_err(|_| format!("Cannot read Firebase credentials at {credentials_path}"))?
    };
    let credentials: FirebaseServiceAccount = serde_json::from_str(&credentials_text)
        .map_err(|_| "Firebase credentials JSON is invalid".to_string())?;
    let now = Utc::now().timestamp();
    let claims = GoogleJwtClaims {
        iss: &credentials.client_email,
        scope: "https://www.googleapis.com/auth/firebase.messaging",
        aud: &credentials.token_uri,
        iat: now,
        exp: now + 3600,
    };
    let assertion = encode(
        &Header::new(Algorithm::RS256),
        &claims,
        &EncodingKey::from_rsa_pem(credentials.private_key.as_bytes())
            .map_err(|_| "Firebase private key is invalid".to_string())?,
    )
    .map_err(|_| "Could not sign Firebase access token".to_string())?;
    let response = reqwest::Client::new()
        .post(&credentials.token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", assertion.as_str()),
        ])
        .send()
        .await
        .map_err(|_| "Could not contact Google OAuth".to_string())?
        .error_for_status()
        .map_err(|_| "Google OAuth rejected the Firebase credentials".to_string())?;
    let token: GoogleTokenResponse = response
        .json()
        .await
        .map_err(|_| "Google OAuth returned an invalid response".to_string())?;
    Ok((credentials.project_id, token.access_token))
}

fn require_command_api_key(headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    let expected_api_key = env::var("COMMAND_API_KEY").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "COMMAND_API_KEY is not configured".to_string(),
        )
    })?;
    let supplied_api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());
    if supplied_api_key == Some(expected_api_key.as_str()) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            "Invalid command API key".to_string(),
        ))
    }
}

async fn read_device_fcm_token(
    state: &AppState,
    device_id: &str,
) -> Result<String, (StatusCode, String)> {
    sqlx::query_scalar::<_, String>(
        "SELECT fcm_token FROM devices WHERE device_id = $1 AND fcm_token IS NOT NULL",
    )
    .bind(device_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read device token".to_string(),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "Device has not registered for Firebase commands".to_string(),
        )
    })
}

async fn require_pending_capability_request(
    state: &AppState,
    device_id: &str,
    capability: &str,
) -> Result<(), (StatusCode, String)> {
    let exists = sqlx::query_scalar::<_, i64>(
        "
        SELECT id::BIGINT
        FROM device_capability_requests
        WHERE device_id = $1
          AND capability = $2
          AND fulfilled_at IS NULL
        ORDER BY requested_at DESC
        LIMIT 1
        ",
    )
    .bind(device_id)
    .bind(capability)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not verify pending upload request".to_string(),
        )
    })?;

    if exists.is_some() {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Upload rejected because no matching backend request is pending".to_string(),
        ))
    }
}

async fn mark_capability_request_fulfilled(
    state: &AppState,
    device_id: &str,
    capability: &str,
) -> Result<(), (StatusCode, String)> {
    sqlx::query(
        "
        UPDATE device_capability_requests
        SET fulfilled_at = NOW()
        WHERE id = (
            SELECT id
            FROM device_capability_requests
            WHERE device_id = $1
              AND capability = $2
              AND fulfilled_at IS NULL
            ORDER BY requested_at DESC
            LIMIT 1
        )
        ",
    )
    .bind(device_id)
    .bind(capability)
    .execute(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not close upload request".to_string(),
        )
    })?;
    Ok(())
}

async fn send_firebase_data_command(
    fcm_token: String,
    data: serde_json::Value,
) -> Result<(), (StatusCode, String)> {
    let (project_id, access_token) = request_firebase_access_token()
        .await
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    let payload = serde_json::json!({
        "message": {
            "token": fcm_token,
            "data": data,
            "android": { "priority": "HIGH" }
        }
    });
    let response = reqwest::Client::new()
        .post(format!(
            "https://fcm.googleapis.com/v1/projects/{project_id}/messages:send"
        ))
        .bearer_auth(access_token)
        .json(&payload)
        .send()
        .await
        .map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                "Could not contact Firebase".to_string(),
            )
        })?;
    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "Firebase returned an unreadable error body".to_string());
    if !status.is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("Firebase rejected the command: HTTP {status}: {body}"),
        ));
    }
    Ok(())
}

async fn request_location_command(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    require_command_api_key(&headers)?;
    println!("Location command requested for device={device_id}");
    let fcm_token = read_device_fcm_token(&state, &device_id).await?;
    send_firebase_data_command(
        fcm_token,
        serde_json::json!({ "command": "locate" }),
    )
    .await?;
    sqlx::query(
        "
        UPDATE devices
        SET last_command_status = 'Location request sent; awaiting the phone',
            last_command_status_time = NOW()
        WHERE device_id = $1
        ",
    )
    .bind(&device_id)
    .execute(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save command status".to_string(),
        )
    })?;
    println!("Firebase accepted location command for device={device_id}");
    Ok(Json(LocateResponse {
        message: format!("Location command sent to {device_id}"),
    }))
}

async fn request_permission_status_command(
    State(state): State<AppState>,
    Path((device_id, permission)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    require_command_api_key(&headers)?;

    let permission_label = match permission.as_str() {
        "camera" => "Camera",
        "microphone" => "Microphone",
        "contacts" => "Contacts",
        "phone_state" => "Phone state",
        "notifications" | "notification" => "Notifications",
        "usage" | "app_usage" | "usage_access" => "Usage stats access",
        _ => return Err((StatusCode::BAD_REQUEST, "Unknown permission".to_string())),
    };

    println!("{permission_label} permission status requested for device={device_id}");
    let fcm_token = read_device_fcm_token(&state, &device_id).await?;
    send_firebase_data_command(
        fcm_token,
        serde_json::json!({
            "command": "permission_status",
            "permission": permission
        }),
    )
    .await?;

    sqlx::query(
        "
        UPDATE devices
        SET last_command_status = $2,
            last_command_status_time = NOW()
        WHERE device_id = $1
        ",
    )
    .bind(&device_id)
    .bind(format!("{permission_label} permission check requested"))
    .execute(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save command status".to_string(),
        )
    })?;

    Ok(Json(LocateResponse {
        message: format!("{permission_label} permission check sent to {device_id}"),
    }))
}

#[derive(Deserialize, Default)]
struct CapabilityQueryParams {
    count: Option<i32>,
    duration_seconds: Option<i64>,
    camera_facing: Option<String>,
}

async fn request_capability_upload_command(
    State(state): State<AppState>,
    Path((device_id, capability)): Path<(String, String)>,
    Query(params): Query<CapabilityQueryParams>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    require_command_api_key(&headers)?;

    let (stored_capability, capability_label, default_camera_facing) = match capability.as_str() {
        "camera" => ("camera", "Camera photo", Some("back")),
        "camera_front" => ("camera", "Front camera photo", Some("front")),
        "camera_back" => ("camera", "Back camera photo", Some("back")),
        "microphone" => ("microphone", "Microphone voice note", None),
        "contacts" => ("contacts", "Contacts", None),
        "call_history" => ("call_history", "Current call history", None),
        "phone_details" | "phone_state" => ("phone_details", "Phone details", None),
        "gallery" | "gallery_photos" => ("gallery", "Gallery photos", None),
        "notifications" | "notification" => ("notifications", "Device notifications", None),
        "app_usage" | "usage" | "screen_time" => ("app_usage", "App usage statistics", None),
        _ => return Err((StatusCode::BAD_REQUEST, "Unknown capability".to_string())),
    };

    let count = params.count.unwrap_or(3);
    let duration_seconds = params.duration_seconds.unwrap_or(5);
    let camera_facing = params.camera_facing.as_deref().or(default_camera_facing).unwrap_or("");

    println!("{capability_label} upload requested for device={device_id} (count={count}, duration={duration_seconds}s)");
    sqlx::query(
        "
        INSERT INTO device_capability_requests (device_id, capability, requested_at)
        VALUES ($1, $2, NOW())
        ",
    )
    .bind(&device_id)
    .bind(stored_capability)
    .execute(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save pending upload request".to_string(),
        )
    })?;

    // Persist first so a fast FCM response can always match a pending request.
    let fcm_token = read_device_fcm_token(&state, &device_id).await?;
    send_firebase_data_command(
        fcm_token,
        serde_json::json!({
            "command": "capability_request",
            "capability": stored_capability,
            "camera_facing": camera_facing,
            "count": count.to_string(),
            "duration_seconds": duration_seconds.to_string()
        }),
    )
    .await?;

    let details_note = if stored_capability == "gallery" {
        format!(" ({count} latest photos)")
    } else if stored_capability == "microphone" {
        format!(" ({duration_seconds}s voice note)")
    } else if stored_capability == "camera" {
        if duration_seconds > 0 {
            format!(" ({count} photos, {duration_seconds}s delay)")
        } else {
            format!(" ({count} photos)")
        }
    } else {
        String::new()
    };

    sqlx::query(
        "
        UPDATE devices
        SET last_command_status = $2,
            last_command_status_time = NOW()
        WHERE device_id = $1
        ",
    )
    .bind(&device_id)
    .bind(format!("{capability_label}{details_note} upload requested; capturing and uploading"))
    .execute(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save command status".to_string(),
        )
    })?;

    Ok(Json(LocateResponse {
        message: format!("{capability_label}{details_note} upload request sent to {device_id}"),
    }))
}

async fn open_app_command(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    require_command_api_key(&headers)?;

    println!("Open app requested for device={device_id}");
    let fcm_token = read_device_fcm_token(&state, &device_id).await?;
    send_firebase_data_command(
        fcm_token,
        serde_json::json!({
            "command": "open_app"
        }),
    )
    .await?;

    sqlx::query(
        "
        UPDATE devices
        SET last_command_status = $2,
            last_command_status_time = NOW()
        WHERE device_id = $1
        ",
    )
    .bind(&device_id)
    .bind("Open app command sent to phone")
    .execute(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save command status".to_string(),
        )
    })?;

    Ok(Json(LocateResponse {
        message: format!("Open app command sent to {device_id}"),
    }))
}

async fn delete_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    let result = sqlx::query("DELETE FROM devices WHERE device_id = $1")
        .bind(&device_id)
        .execute(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not delete device".to_string(),
            )
        })?;
    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Device was not found".to_string()));
    }
    println!("Device deleted from dashboard: device={device_id}");
    Ok(Json(LocateResponse {
        message: format!("Device {device_id} deleted"),
    }))
}

async fn delete_detail_contact(
    State(state): State<AppState>,
    Path(path): Path<DeleteItemPath>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    let result = sqlx::query("DELETE FROM device_contacts WHERE device_id = $1 AND id = $2")
        .bind(&path.device_id)
        .bind(path.id)
        .execute(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not delete contact".to_string(),
            )
        })?;
    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Contact was not found".to_string()));
    }
    Ok(Json(LocateResponse {
        message: "Contact deleted".to_string(),
    }))
}

async fn delete_detail_media(
    State(state): State<AppState>,
    Path(path): Path<DeleteItemPath>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    let result = sqlx::query("DELETE FROM device_media WHERE device_id = $1 AND id = $2")
        .bind(&path.device_id)
        .bind(path.id)
        .execute(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not delete media".to_string(),
            )
        })?;
    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Media item was not found".to_string()));
    }
    Ok(Json(LocateResponse {
        message: "Media item deleted".to_string(),
    }))
}

async fn delete_detail_notification(
    State(state): State<AppState>,
    Path(path): Path<DeleteItemPath>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    let result = sqlx::query("DELETE FROM device_notifications WHERE device_id = $1 AND id = $2")
        .bind(&path.device_id)
        .bind(path.id)
        .execute(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not delete notification".to_string(),
            )
        })?;
    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Notification was not found".to_string()));
    }
    Ok(Json(LocateResponse {
        message: "Notification deleted".to_string(),
    }))
}

async fn delete_detail_alert(
    State(state): State<AppState>,
    Path(path): Path<DeleteItemPath>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    let result = sqlx::query("DELETE FROM device_alerts WHERE device_id = $1 AND id = $2")
        .bind(&path.device_id)
        .bind(path.id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete alert".to_string()))?;
    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Alert was not found".to_string()));
    }
    Ok(Json(LocateResponse {
        message: "Alert deleted".to_string(),
    }))
}

async fn delete_detail_app_usage(
    State(state): State<AppState>,
    Path(path): Path<DeleteItemPath>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    let result = sqlx::query("DELETE FROM device_app_usage WHERE device_id = $1 AND id = $2")
        .bind(&path.device_id)
        .bind(path.id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete app usage item".to_string()))?;
    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "App usage item was not found".to_string()));
    }
    Ok(Json(LocateResponse {
        message: "App usage item deleted".to_string(),
    }))
}

async fn delete_detail_scope(
    State(state): State<AppState>,
    Path(path): Path<DeleteScopePath>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    match path.scope.as_str() {
        "photos" => {
            sqlx::query("DELETE FROM device_media WHERE device_id = $1 AND media_type = 'photo'")
                .bind(&path.device_id)
                .execute(&state.db)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Could not delete photos".to_string(),
                    )
                })?;
        }
        "voice" => {
            sqlx::query("DELETE FROM device_media WHERE device_id = $1 AND media_type = 'voice'")
                .bind(&path.device_id)
                .execute(&state.db)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Could not delete voice notes".to_string(),
                    )
                })?;
        }
        "contacts" => {
            sqlx::query("DELETE FROM device_contacts WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&state.db)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Could not delete contacts".to_string(),
                    )
                })?;
        }
        "call_history" => {
            sqlx::query("DELETE FROM device_call_history WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&state.db)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete call history".to_string()))?;
        }
        "notifications" => {
            sqlx::query("DELETE FROM device_notifications WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&state.db)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete notifications".to_string()))?;
        }
        "alerts" => {
            sqlx::query("DELETE FROM device_alerts WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&state.db)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete alerts".to_string()))?;
        }
        "app_usage" => {
            sqlx::query("DELETE FROM device_app_usage WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&state.db)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete app usage".to_string()))?;
        }
        "geofences" => {
            sqlx::query("DELETE FROM device_geofences WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&state.db)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete geofences".to_string()))?;
        }
        "all" => {
            let mut transaction = state.db.begin().await.map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not start delete transaction".to_string(),
                )
            })?;
            sqlx::query("DELETE FROM device_media WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&mut *transaction)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Could not delete saved media".to_string(),
                    )
                })?;
            sqlx::query("DELETE FROM device_contacts WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&mut *transaction)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Could not delete saved contacts".to_string(),
                    )
                })?;
            sqlx::query("DELETE FROM device_call_history WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&mut *transaction)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete saved call history".to_string()))?;
            sqlx::query("DELETE FROM device_notifications WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&mut *transaction)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete saved notifications".to_string()))?;
            sqlx::query("DELETE FROM device_alerts WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&mut *transaction)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete saved alerts".to_string()))?;
            sqlx::query("DELETE FROM device_app_usage WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&mut *transaction)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete saved app usage".to_string()))?;
            sqlx::query("DELETE FROM device_geofences WHERE device_id = $1")
                .bind(&path.device_id)
                .execute(&mut *transaction)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not delete saved geofences".to_string()))?;
            transaction.commit().await.map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not finish delete transaction".to_string(),
                )
            })?;
        }
        _ => return Err((StatusCode::BAD_REQUEST, "Unsupported delete scope".to_string())),
    }
    Ok(Json(LocateResponse {
        message: format!("Deleted {}", path.scope),
    }))
}

async fn device_details_page(
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Result<Html<String>, DashboardAuthError> {
    require_dashboard_auth(&headers)?;
    Ok(Html(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Device details</title>
  <style>
    :root {{ color-scheme: dark; font-family: Inter,system-ui,sans-serif; }}
    body {{ margin: 0; background: #10131a; color: #eef2ff; }}
    main {{ max-width: 1120px; margin: 0 auto; padding: 32px 20px; }}
    a {{ color: #75a1ff; }} .muted {{ color: #aab5cd; }}
    section {{ margin: 18px 0; padding: 16px; background: #171d29; border: 1px solid #30394c; border-radius: 10px; }}
    table {{ width: 100%; border-collapse: collapse; }} th,td {{ text-align: left; padding: 10px; border-bottom: 1px solid #30394c; vertical-align: top; }}
    img {{ max-width: 260px; border-radius: 8px; display: block; margin: 8px 0; }}
    audio {{ width: 280px; max-width: 100%; }}
    button {{ min-height: 34px; padding: 8px 11px; border: 0; border-radius: 8px; background: #253047; color: #eef2ff; cursor: pointer; font-weight: 700; }}
    button.danger {{ background: #d75067; }}
    .section-head {{ display: flex; justify-content: space-between; gap: 12px; align-items: center; margin-bottom: 10px; }}
    .media-grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 14px; }}
    .media-card {{ padding: 12px; border: 1px solid #30394c; border-radius: 8px; background: #121824; }}
    .item-actions {{ margin-top: 8px; }}
    #message {{ min-height: 20px; color: #aab5cd; }}
  </style>
</head>
<body>
  <main>
    <a href="/">Back to dashboard</a>
    <h1>Device details</h1>
    <p class="muted" id="device-id"></p>
    <section id="phone"><div class="section-head"><h2>Phone & Hardware Status</h2><button class="danger" onclick="deleteScope('all')">Delete All Saved Data</button></div><div id="phone-content">Loading...</div></section>
    <section id="sim"><div class="section-head"><h2>📶 SIM Card & Cellular Telemetry</h2><button class="danger" onclick="deleteScope('sim_history')">Delete SIM History</button></div><div id="sim-content">Loading...</div></section>
    <section id="alerts"><div class="section-head"><h2>Live Alerts & Security Events</h2><button class="danger" onclick="deleteScope('alerts')">Delete All Alerts</button></div><div id="alerts-content">Loading...</div></section>
    <section id="geofences"><div class="section-head"><h2>Geofence Safety Zones</h2><button class="danger" onclick="deleteScope('geofences')">Delete All Geofences</button></div><div id="geofence-form" style="margin-bottom:12px;display:flex;gap:8px;flex-wrap:wrap;align-items:center;"><input id="geo-name" placeholder="Zone name (e.g. Home)" style="padding:6px 8px;border-radius:6px;border:1px solid #30394c;background:#121824;color:#eef2ff;"><input id="geo-lat" type="number" step="any" placeholder="Latitude" style="width:110px;padding:6px 8px;border-radius:6px;border:1px solid #30394c;background:#121824;color:#eef2ff;"><input id="geo-lon" type="number" step="any" placeholder="Longitude" style="width:110px;padding:6px 8px;border-radius:6px;border:1px solid #30394c;background:#121824;color:#eef2ff;"><input id="geo-radius" type="number" placeholder="Radius (meters)" value="200" style="width:110px;padding:6px 8px;border-radius:6px;border:1px solid #30394c;background:#121824;color:#eef2ff;"><button onclick="addGeofence()">+ Add Geofence</button></div><div id="geofences-content">Loading...</div></section>
    <section id="app_usage"><div class="section-head"><h2>App Usage & Screen Time</h2><button class="danger" onclick="deleteScope('app_usage')">Delete App Usage</button></div><div id="usage-content">Loading...</div></section>
    <section id="notifications"><div class="section-head"><h2>Device Notifications</h2><button class="danger" onclick="deleteScope('notifications')">Delete All Notifications</button></div><div id="notifications-content">Loading...</div></section>
    <section id="contacts"><div class="section-head"><h2>Contacts Shared By User</h2><button class="danger" onclick="deleteScope('contacts')">Delete All Contacts</button></div><div id="contacts-content">Loading...</div></section>
    <section id="calls"><div class="section-head"><h2>Current Call History</h2><button class="danger" onclick="deleteScope('call_history')">Delete Call History</button></div><div id="calls-content">Loading...</div></section>
    <section id="photos"><div class="section-head"><h2>Photos Shared By User</h2><button class="danger" onclick="deleteScope('photos')">Delete All Photos</button></div><div id="photos-content">Loading...</div></section>
    <section id="voice"><div class="section-head"><h2>Voice Notes Shared By User</h2><button class="danger" onclick="deleteScope('voice')">Delete All Voice</button></div><div id="voice-content">Loading...</div></section>
  </main>
  <script>
    const deviceId = {device_id:?};
    const byId = id => document.getElementById(id);
    const message = value => byId('message').textContent = value || '';
    const escape = value => {{
      const span = document.createElement('span');
      span.textContent = value ?? '-';
      return span.innerHTML;
    }};
    const text = value => escape(value);
    let didInitialScroll = false;
    document.getElementById('device-id').textContent = deviceId;
    function audioIsPlaying() {{
      return Array.from(document.querySelectorAll('audio')).some(audio => !audio.paused && !audio.ended);
    }}
    function scrollToRequestedSection() {{
      if (didInitialScroll || !location.hash) return;
      const target = document.getElementById(location.hash.slice(1));
      if (!target) return;
      didInitialScroll = true;
      requestAnimationFrame(() => target.scrollIntoView({{behavior:'smooth', block:'start'}}));
    }}
    function formatDuration(ms) {{
      const sec = Math.floor(ms / 1000);
      const hrs = Math.floor(sec / 3600);
      const mins = Math.floor((sec % 3600) / 60);
      const remSec = sec % 60;
      if (hrs > 0) return `${{hrs}}h ${{mins}}m`;
      if (mins > 0) return `${{mins}}m ${{remSec}}s`;
      return `${{remSec}}s`;
    }}
    async function load() {{
      if (audioIsPlaying()) return;
      const response = await fetch('/devices/'+encodeURIComponent(deviceId)+'/details-data?t='+Date.now(), {{cache:'no-store'}});
      const data = await response.json();
      const screenBadge = data.device.screen_state ? `<span style="padding:3px 8px;border-radius:6px;font-weight:700;background:${{data.device.screen_state.includes('Unlocked')?'#166534':data.device.screen_state.includes('Locked')?'#854d0e':'#374151'}};color:#f0fdf4;">${{text(data.device.screen_state)}}</span>` : '-';
      byId('phone-content').innerHTML = `<table><tr><th>Screen State</th><td>${{screenBadge}}</td></tr><tr><th>Manufacturer</th><td>${{text(data.device.manufacturer)}}</td></tr><tr><th>Model</th><td>${{text(data.device.model)}}</td></tr><tr><th>Android</th><td>${{text(data.device.android_version)}}</td></tr><tr><th>Last seen</th><td>${{text(data.device.last_seen)}}</td></tr></table>`;
      
      const currentSim = `<table><tr><th>Active Carrier</th><td><strong>${{text(data.device.sim_carrier)}}</strong></td></tr><tr><th>SIM Operator</th><td>${{text(data.device.sim_operator)}}</td></tr><tr><th>SIM Phone Number</th><td><strong style="color:#38bdf8;">${{text(data.device.sim_number)}}</strong></td></tr><tr><th>Country ISO</th><td>${{text(data.device.sim_country)}}</td></tr><tr><th>SIM Serial / ICCID</th><td><code>${{text(data.device.sim_serial)}}</code></td></tr></table>`;
      const simHistoryRows = data.sim_history && data.sim_history.length ? `<h4>SIM Swap / State History</h4><table><thead><tr><th>When</th><th>Event</th><th>Carrier</th><th>Number</th><th>Serial</th></tr></thead><tbody>${{data.sim_history.map(s=>`<tr><td>${{new Date(s.created_at).toLocaleString()}}</td><td><span style="padding:2px 6px;border-radius:4px;font-size:12px;background:#1e293b;">${{text(s.event_type)}}</span></td><td><strong>${{text(s.sim_carrier)}}</strong></td><td>${{text(s.sim_number)}}</td><td><code>${{text(s.sim_serial)}}</code></td></tr>`).join('')}}</tbody></table>` : '<p class="muted" style="margin-top:12px;">No SIM swap events recorded yet.</p>';
      byId('sim-content').innerHTML = currentSim + simHistoryRows;
      
      byId('alerts-content').innerHTML = data.alerts && data.alerts.length ? `<table><thead><tr><th>Severity</th><th>Event</th><th>Details</th><th>When</th><th>Action</th></tr></thead><tbody>${{data.alerts.map(a=>`<tr><td><span style="padding:3px 8px;border-radius:4px;font-weight:700;font-size:12px;background:${{a.severity==='critical'?'#d75067':a.severity==='warning'?'#eab308':'#3b82f6'}}">${{text(a.severity)}}</span></td><td><strong>${{text(a.title)}}</strong></td><td>${{text(a.message)}}</td><td>${{new Date(a.created_at).toLocaleString()}}</td><td><button class="danger" onclick="deleteAlert(${{a.id}})">Delete</button></td></tr>`).join('')}}</tbody></table>` : '<p class="muted">No alerts or security events logged yet.</p>';
      
      byId('geofences-content').innerHTML = data.geofences && data.geofences.length ? `<table><thead><tr><th>Name</th><th>Center (Lat, Lon)</th><th>Radius</th><th>Status</th><th>Action</th></tr></thead><tbody>${{data.geofences.map(g=>`<tr><td><strong>${{text(g.name)}}</strong></td><td>${{g.latitude.toFixed(5)}}, ${{g.longitude.toFixed(5)}}</td><td>${{g.radius_meters}} m</td><td>${{g.is_active ? '<span style="color:#4ade80">Active</span>' : '<span style="color:#aab5cd">Inactive</span>'}}</td><td><button class="danger" onclick="deleteGeofenceItem(${{g.id}})">Delete</button></td></tr>`).join('')}}</tbody></table>` : '<p class="muted">No geofences created yet. Use the form above to add a safety zone.</p>';
      
      byId('usage-content').innerHTML = data.app_usage && data.app_usage.length ? `<table><thead><tr><th>App Name</th><th>Foreground Screen Time</th><th>Last Used</th><th>Action</th></tr></thead><tbody>${{data.app_usage.map(u=>`<tr><td><strong>${{text(u.app_name)}}</strong><br><small class="muted">${{text(u.package_name)}}</small></td><td><strong>${{formatDuration(u.total_time_foreground_ms)}}</strong></td><td>${{new Date(u.last_time_used_ms).toLocaleString()}}</td><td><button class="danger" onclick="deleteAppUsageItem(${{u.id}})">Delete</button></td></tr>`).join('')}}</tbody></table>` : '<p class="muted">No app usage stats uploaded yet.</p>';

      byId('notifications-content').innerHTML = data.notifications && data.notifications.length ? `<table><thead><tr><th>App</th><th>Title</th><th>Message / Content</th><th>When</th><th>Action</th></tr></thead><tbody>${{data.notifications.map(n=>`<tr><td><strong>${{text(n.app_name || n.package_name)}}</strong><br><small class="muted">${{text(n.package_name)}}</small></td><td>${{text(n.title)}}</td><td>${{text(n.text)}}</td><td>${{new Date(n.post_time_ms).toLocaleString()}}</td><td><button class="danger" onclick="deleteNotification(${{n.id}})">Delete</button></td></tr>`).join('')}}</tbody></table>` : '<p class="muted">No notifications uploaded yet.</p>';
      byId('contacts-content').innerHTML = data.contacts.length ? `<table><thead><tr><th>Name</th><th>Phone</th><th>Action</th></tr></thead><tbody>${{data.contacts.map(c=>`<tr><td>${{text(c.name)}}</td><td>${{text(c.phone)}}</td><td><button class="danger" onclick="deleteContact(${{c.id}})">Delete</button></td></tr>`).join('')}}</tbody></table>` : '<p class="muted">No contacts uploaded yet.</p>';
      const callCounts = data.calls.reduce((counts, call) => {{ const key = call.phone_number || call.cached_name || 'unknown'; counts[key] = (counts[key] || 0) + 1; return counts; }}, {{}});
      byId('calls-content').innerHTML = data.calls.length ? `<table><thead><tr><th>Name / Number</th><th>Type</th><th>When</th><th>Duration</th><th>Calls in snapshot</th></tr></thead><tbody>${{data.calls.map(c=>{{ const key = c.phone_number || c.cached_name || 'unknown'; return `<tr><td>${{text(c.cached_name || c.phone_number)}}</td><td>${{text(c.call_type)}}</td><td>${{new Date(c.called_at_ms).toLocaleString()}}</td><td>${{Math.floor(c.duration_seconds / 60)}}m ${{c.duration_seconds % 60}}s</td><td>${{callCounts[key]}}</td></tr>`; }}).join('')}}</tbody></table>` : '<p class="muted">No call-history snapshot uploaded yet.</p>';
      const photos = data.media.filter(m=>m.media_type==='photo');
      byId('photos-content').innerHTML = photos.length ? `<div class="media-grid">${{photos.map(m=>`<div class="media-card"><small>${{text(m.created_at)}}</small><img src="data:${{text(m.content_type)}};base64,${{m.base64_data}}" alt="Shared photo"><div class="item-actions"><button class="danger" onclick="deleteMedia(${{m.id}})">Delete</button></div></div>`).join('')}}</div>` : '<p class="muted">No photos uploaded yet.</p>';
      const voice = data.media.filter(m=>m.media_type==='voice');
      byId('voice-content').innerHTML = voice.length ? `<div class="media-grid">${{voice.map(m=>`<div class="media-card"><small>${{text(m.created_at)}}</small><br><audio controls preload="metadata" src="data:${{text(m.content_type)}};base64,${{m.base64_data}}"></audio><div class="item-actions"><button class="danger" onclick="deleteMedia(${{m.id}})">Delete</button></div></div>`).join('')}}</div>` : '<p class="muted">No voice notes uploaded yet.</p>';
      scrollToRequestedSection();
    }}
    async function deleteRequest(url, label) {{
      if (!confirm(label)) return;
      const response = await fetch(url, {{method:'DELETE', cache:'no-store'}});
      if (!response.ok) {{
        message(await response.text() || 'Delete failed');
        return;
      }}
      message('Deleted. Refreshing...');
      await load();
      setTimeout(() => message(''), 1500);
    }}
    function deleteAlert(id) {{
      deleteRequest('/devices/'+encodeURIComponent(deviceId)+'/details/alerts/'+id, 'Delete this alert?');
    }}
    function deleteGeofenceItem(id) {{
      deleteRequest('/devices/'+encodeURIComponent(deviceId)+'/geofences/'+id, 'Delete this geofence?');
    }}
    function deleteAppUsageItem(id) {{
      deleteRequest('/devices/'+encodeURIComponent(deviceId)+'/details/app-usage/'+id, 'Delete this app usage item?');
    }}
    function deleteNotification(id) {{
      deleteRequest('/devices/'+encodeURIComponent(deviceId)+'/details/notifications/'+id, 'Delete this notification?');
    }}
    function deleteContact(id) {{
      deleteRequest('/devices/'+encodeURIComponent(deviceId)+'/details/contacts/'+id, 'Delete this contact?');
    }}
    function deleteMedia(id) {{
      deleteRequest('/devices/'+encodeURIComponent(deviceId)+'/details/media/'+id, 'Delete this saved item?');
    }}
    function deleteScope(scope) {{
      const labels = {{photos:'all photos', voice:'all voice notes', contacts:'all contacts', call_history:'current call history', notifications:'all notifications', alerts:'all alerts', app_usage:'all app usage stats', geofences:'all geofences', all:'all saved data'}};
      deleteRequest('/devices/'+encodeURIComponent(deviceId)+'/details/'+scope, 'Delete '+(labels[scope] || scope)+'?');
    }}
    async function addGeofence() {{
      const name = byId('geo-name').value.trim();
      const lat = parseFloat(byId('geo-lat').value);
      const lon = parseFloat(byId('geo-lon').value);
      const radius = parseFloat(byId('geo-radius').value) || 200;
      if (!name || isNaN(lat) || isNaN(lon)) {{
        alert('Please fill out Name, Latitude, and Longitude');
        return;
      }}
      const response = await fetch('/devices/'+encodeURIComponent(deviceId)+'/geofences', {{
        method: 'POST',
        headers: {{ 'Content-Type': 'application/json' }},
        body: JSON.stringify({{ name, latitude: lat, longitude: lon, radius_meters: radius }})
      }});
      if (!response.ok) {{
        message('Failed to add geofence');
        return;
      }}
      byId('geo-name').value = '';
      message('Geofence added');
      await load();
      setTimeout(() => message(''), 1500);
    }}
    load();
    setInterval(load, 3000);
  </script>
</body>
</html>"#,
    )))
}

async fn device_details_data(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, DashboardAuthError> {
    require_dashboard_auth(&headers)?;
    let device = sqlx::query(
        "
        SELECT device_id, manufacturer, model, android_version, last_seen
        FROM devices
        WHERE device_id = $1
        ",
    )
    .bind(&device_id)
    .fetch_optional(&state.db)
    .await
    .unwrap();
    let Some(device) = device else {
        return Ok(Json(serde_json::json!({
            "device": { "device_id": device_id },
            "alerts": [],
            "geofences": [],
            "app_usage": [],
            "notifications": [],
            "contacts": [],
            "calls": [],
            "media": []
        })));
    };
    let alerts = sqlx::query(
        "SELECT id, alert_type, severity, title, message, created_at FROM device_alerts WHERE device_id = $1 ORDER BY created_at DESC LIMIT 100",
    )
    .bind(&device_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let geofences = sqlx::query(
        "SELECT id, name, latitude, longitude, radius_meters, is_active FROM device_geofences WHERE device_id = $1 ORDER BY created_at DESC",
    )
    .bind(&device_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let app_usage = sqlx::query(
        "SELECT id, package_name, app_name, total_time_foreground_ms, last_time_used_ms FROM device_app_usage WHERE device_id = $1 ORDER BY total_time_foreground_ms DESC LIMIT 100",
    )
    .bind(&device_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let notifications = sqlx::query(
        "SELECT id, package_name, app_name, title, text, post_time_ms FROM device_notifications WHERE device_id = $1 ORDER BY post_time_ms DESC LIMIT 200",
    )
    .bind(&device_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let contacts = sqlx::query(
        "
        SELECT id, name, phone
        FROM device_contacts
        WHERE device_id = $1
        ORDER BY COALESCE(name, phone)
        ",
    )
    .bind(&device_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let media = sqlx::query(
        "
        SELECT id, media_type, content_type, base64_data, created_at
        FROM device_media
        WHERE device_id = $1
        ORDER BY created_at DESC
        LIMIT 100
        ",
    )
    .bind(&device_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let calls = sqlx::query(
        "SELECT cached_name, phone_number, call_type, called_at_ms, duration_seconds FROM device_call_history WHERE device_id = $1 ORDER BY called_at_ms DESC LIMIT 100",
    )
    .bind(&device_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    Ok(Json(serde_json::json!({
        "device": {
            "device_id": device.get::<String, _>("device_id"),
            "manufacturer": device.get::<Option<String>, _>("manufacturer"),
            "model": device.get::<Option<String>, _>("model"),
            "android_version": device.get::<Option<String>, _>("android_version"),
            "last_seen": device.get::<Option<chrono::NaiveDateTime>, _>("last_seen")
        },
        "alerts": alerts.into_iter().map(|row| serde_json::json!({
            "id": row.get::<i32, _>("id"),
            "alert_type": row.get::<String, _>("alert_type"),
            "severity": row.get::<String, _>("severity"),
            "title": row.get::<String, _>("title"),
            "message": row.get::<String, _>("message"),
            "created_at": row.get::<chrono::NaiveDateTime, _>("created_at")
        })).collect::<Vec<_>>(),
        "geofences": geofences.into_iter().map(|row| serde_json::json!({
            "id": row.get::<i32, _>("id"),
            "name": row.get::<String, _>("name"),
            "latitude": row.get::<f64, _>("latitude"),
            "longitude": row.get::<f64, _>("longitude"),
            "radius_meters": row.get::<f64, _>("radius_meters"),
            "is_active": row.get::<bool, _>("is_active")
        })).collect::<Vec<_>>(),
        "app_usage": app_usage.into_iter().map(|row| serde_json::json!({
            "id": row.get::<i32, _>("id"),
            "package_name": row.get::<String, _>("package_name"),
            "app_name": row.get::<String, _>("app_name"),
            "total_time_foreground_ms": row.get::<i64, _>("total_time_foreground_ms"),
            "last_time_used_ms": row.get::<i64, _>("last_time_used_ms")
        })).collect::<Vec<_>>(),
        "notifications": notifications.into_iter().map(|row| serde_json::json!({
            "id": row.get::<i32, _>("id"),
            "package_name": row.get::<Option<String>, _>("package_name"),
            "app_name": row.get::<Option<String>, _>("app_name"),
            "title": row.get::<Option<String>, _>("title"),
            "text": row.get::<Option<String>, _>("text"),
            "post_time_ms": row.get::<i64, _>("post_time_ms")
        })).collect::<Vec<_>>(),
        "contacts": contacts.into_iter().map(|row| serde_json::json!({
            "id": row.get::<i32, _>("id"),
            "name": row.get::<Option<String>, _>("name"),
            "phone": row.get::<Option<String>, _>("phone")
        })).collect::<Vec<_>>(),
        "calls": calls.into_iter().map(|row| serde_json::json!({
            "cached_name": row.get::<Option<String>, _>("cached_name"),
            "phone_number": row.get::<Option<String>, _>("phone_number"),
            "call_type": row.get::<String, _>("call_type"),
            "called_at_ms": row.get::<i64, _>("called_at_ms"),
            "duration_seconds": row.get::<i64, _>("duration_seconds")
        })).collect::<Vec<_>>(),
        "media": media.into_iter().map(|row| serde_json::json!({
            "id": row.get::<i32, _>("id"),
            "media_type": row.get::<String, _>("media_type"),
            "content_type": row.get::<String, _>("content_type"),
            "base64_data": row.get::<String, _>("base64_data"),
            "created_at": row.get::<chrono::NaiveDateTime, _>("created_at")
        })).collect::<Vec<_>>(),
        "sim_history": sqlx::query(
            "SELECT id, sim_carrier, sim_operator, sim_number, sim_country, sim_serial, event_type, created_at FROM device_sim_history WHERE device_id = $1 ORDER BY created_at DESC LIMIT 50",
        )
        .bind(&device_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| serde_json::json!({
            "id": row.get::<i32, _>("id"),
            "sim_carrier": row.get::<Option<String>, _>("sim_carrier"),
            "sim_operator": row.get::<Option<String>, _>("sim_operator"),
            "sim_number": row.get::<Option<String>, _>("sim_number"),
            "sim_country": row.get::<Option<String>, _>("sim_country"),
            "sim_serial": row.get::<Option<String>, _>("sim_serial"),
            "event_type": row.get::<String, _>("event_type"),
            "created_at": row.get::<chrono::NaiveDateTime, _>("created_at")
        })).collect::<Vec<_>>()
    })))
}

async fn list_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Device>>, DashboardAuthError> {
    require_dashboard_auth(&headers)?;
    let devices = sqlx::query_as::<_, Device>(
        "
        SELECT
          device_id,
          manufacturer,
          model,
          android_version,
          screen_state,
          sim_operator,
          sim_carrier,
          sim_number,
          sim_country,
          sim_serial,
          latitude, 
          longitude,
          location_accuracy_meters,
          location_time,
          last_command_status,
          last_command_status_time,
          last_seen,
          (fcm_token IS NOT NULL) AS command_ready,
          COALESCE(last_seen > NOW() - INTERVAL '5 seconds',false) AS online 
        FROM devices
        ",
    )
    .fetch_all(&state.db)
    .await
    .unwrap();
    Ok(Json(devices))
}

async fn list_online_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Device>>, DashboardAuthError> {
    require_dashboard_auth(&headers)?;
    let devices = sqlx::query_as::<_, Device>(
        "
        SELECT
          device_id,
          manufacturer,
          model,
          android_version,
          screen_state,
          sim_operator,
          sim_carrier,
          sim_number,
          sim_country,
          sim_serial,
          latitude,
          longitude,
          location_accuracy_meters,
          location_time,
          last_command_status,
          last_command_status_time,
          last_seen,
          (fcm_token IS NOT NULL) AS command_ready,
          true AS online
        FROM devices
        WHERE last_seen > NOW() - INTERVAL '5 seconds'
        ",
    )
    .fetch_all(&state.db)
    .await
    .unwrap();
    Ok(Json(devices))
}

#[tokio::main]
async fn main() {
    let db = connect_db().await;

    sqlx::query(
        "
        CREATE TABLE IF NOT EXISTS devices (
            device_id TEXT PRIMARY KEY,
            manufacturer TEXT,
            model TEXT,
            android_version TEXT,
            screen_state TEXT,
            sim_operator TEXT,
            sim_carrier TEXT,
            sim_number TEXT,
            sim_country TEXT,
            sim_serial TEXT,
            latitude DOUBLE PRECISION,
            longitude DOUBLE PRECISION,
            location_accuracy_meters DOUBLE PRECISION,
            location_time TIMESTAMP,
            last_command_status TEXT,
            last_command_status_time TIMESTAMP,
            fcm_token TEXT,
            last_seen TIMESTAMP
        )
        ",
    )
    .execute(&db)
    .await
    .expect("Failed to ensure devices table");

    sqlx::query(
        "
        ALTER TABLE devices
        ADD COLUMN IF NOT EXISTS manufacturer TEXT,
        ADD COLUMN IF NOT EXISTS model TEXT,
        ADD COLUMN IF NOT EXISTS android_version TEXT,
        ADD COLUMN IF NOT EXISTS screen_state TEXT,
        ADD COLUMN IF NOT EXISTS sim_operator TEXT,
        ADD COLUMN IF NOT EXISTS sim_carrier TEXT,
        ADD COLUMN IF NOT EXISTS sim_number TEXT,
        ADD COLUMN IF NOT EXISTS sim_country TEXT,
        ADD COLUMN IF NOT EXISTS sim_serial TEXT,
        ADD COLUMN IF NOT EXISTS latitude DOUBLE PRECISION,
        ADD COLUMN IF NOT EXISTS longitude DOUBLE PRECISION,
        ADD COLUMN IF NOT EXISTS location_accuracy_meters DOUBLE PRECISION,
        ADD COLUMN IF NOT EXISTS location_time TIMESTAMP,
        ADD COLUMN IF NOT EXISTS last_command_status TEXT,
        ADD COLUMN IF NOT EXISTS last_command_status_time TIMESTAMP,
        ADD COLUMN IF NOT EXISTS fcm_token TEXT
        ",
    )
    .execute(&db)
    .await
    .expect("Failed to ensure device info columns");

    sqlx::query(
        "
        CREATE TABLE IF NOT EXISTS device_sim_history (
            id SERIAL PRIMARY KEY,
            device_id TEXT NOT NULL,
            sim_carrier TEXT,
            sim_operator TEXT,
            sim_number TEXT,
            sim_country TEXT,
            sim_serial TEXT,
            event_type TEXT NOT NULL DEFAULT 'snapshot',
            created_at TIMESTAMP NOT NULL DEFAULT NOW()
        )
        ",
    )
    .execute(&db)
    .await
    .expect("Failed to ensure device sim history table");

    sqlx::query(
        "
        CREATE TABLE IF NOT EXISTS device_contacts (
            id SERIAL PRIMARY KEY,
            device_id TEXT NOT NULL,
            name TEXT,
            phone TEXT
        )
        ",
    )
    .execute(&db)
    .await
    .expect("Failed to ensure device contacts table");

    sqlx::query(
        "
        CREATE TABLE IF NOT EXISTS device_media (
            id SERIAL PRIMARY KEY,
            device_id TEXT NOT NULL,
            media_type TEXT NOT NULL,
            content_type TEXT NOT NULL,
            base64_data TEXT NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT NOW()
        )
        ",
    )
    .execute(&db)
    .await
    .expect("Failed to ensure device media table");

    sqlx::query(
        "
        CREATE TABLE IF NOT EXISTS device_capability_requests (
            id SERIAL PRIMARY KEY,
            device_id TEXT NOT NULL,
            capability TEXT NOT NULL,
            requested_at TIMESTAMP NOT NULL DEFAULT NOW(),
            fulfilled_at TIMESTAMP
        )
        ",
    )
    .execute(&db)
    .await
    .expect("Failed to ensure device capability requests table");

    sqlx::query(
        "
        CREATE TABLE IF NOT EXISTS device_call_history (
            id SERIAL PRIMARY KEY,
            device_id TEXT NOT NULL,
            cached_name TEXT,
            phone_number TEXT,
            call_type TEXT NOT NULL,
            called_at_ms BIGINT NOT NULL,
            duration_seconds BIGINT NOT NULL
        )
        ",
    )
    .execute(&db)
    .await
    .expect("Failed to ensure device call-history table");

    sqlx::query(
        "
        CREATE TABLE IF NOT EXISTS device_notifications (
            id SERIAL PRIMARY KEY,
            device_id TEXT NOT NULL,
            package_name TEXT,
            app_name TEXT,
            title TEXT,
            text TEXT,
            post_time_ms BIGINT NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT NOW()
        )
        ",
    )
    .execute(&db)
    .await
    .expect("Failed to ensure device notifications table");

    sqlx::query(
        "
        CREATE TABLE IF NOT EXISTS device_alerts (
            id SERIAL PRIMARY KEY,
            device_id TEXT NOT NULL,
            alert_type TEXT NOT NULL,
            severity TEXT NOT NULL DEFAULT 'info',
            title TEXT NOT NULL,
            message TEXT NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT NOW()
        )
        ",
    )
    .execute(&db)
    .await
    .expect("Failed to ensure device alerts table");

    sqlx::query(
        "
        CREATE TABLE IF NOT EXISTS device_geofences (
            id SERIAL PRIMARY KEY,
            device_id TEXT NOT NULL,
            name TEXT NOT NULL,
            latitude DOUBLE PRECISION NOT NULL,
            longitude DOUBLE PRECISION NOT NULL,
            radius_meters DOUBLE PRECISION NOT NULL,
            is_active BOOLEAN NOT NULL DEFAULT TRUE,
            created_at TIMESTAMP NOT NULL DEFAULT NOW()
        )
        ",
    )
    .execute(&db)
    .await
    .expect("Failed to ensure device geofences table");

    sqlx::query(
        "
        CREATE TABLE IF NOT EXISTS device_app_usage (
            id SERIAL PRIMARY KEY,
            device_id TEXT NOT NULL,
            package_name TEXT NOT NULL,
            app_name TEXT NOT NULL,
            total_time_foreground_ms BIGINT NOT NULL,
            last_time_used_ms BIGINT NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT NOW()
        )
        ",
    )
    .execute(&db)
    .await
    .expect("Failed to ensure device app usage table");

    let state = AppState { db }; //Stores the DB pool in app State

    let app = Router::new() //creates the HTTP router

    //ALL this will connect URLs to rust function
        .route("/", get(dashboard))
        .route("/health", get(health))
        .route("/register", post(register))
        .route("/devices", get(list_devices))
        .route("/devices/online", get(list_online_devices))
        .route("/devices/fcm-token", post(register_fcm_token))
        .route("/devices/location-status", post(update_location_command_status))
        .route("/devices/phone-details", post(update_phone_details))
        .route("/devices/contacts", post(upload_contacts))
        .route("/devices/call-history", post(upload_call_history))
        .route("/devices/notifications", post(upload_notifications))
        .route("/devices/alerts", post(upload_device_alert))
        .route("/devices/geofences/event", post(upload_geofence_event))
        .route("/devices/{device_id}/geofences", post(create_geofence).get(list_geofences))
        .route("/devices/{device_id}/geofences-query", post(query_geofences))
        .route("/devices/{device_id}/geofences/{id}", delete(delete_geofence))
        .route("/devices/app-usage", post(upload_app_usage))
        .route("/devices/media", post(upload_media))
        .route("/devices/{device_id}/details", get(device_details_page))
        .route("/devices/{device_id}/details-data", get(device_details_data))
        .route(
            "/devices/{device_id}/details/{scope}",
            delete(delete_detail_scope),
        )
        .route(
            "/devices/{device_id}/details/contacts/{id}",
            delete(delete_detail_contact),
        )
        .route(
            "/devices/{device_id}/details/media/{id}",
            delete(delete_detail_media),
        )
        .route(
            "/devices/{device_id}/details/notifications/{id}",
            delete(delete_detail_notification),
        )
        .route(
            "/devices/{device_id}/details/alerts/{id}",
            delete(delete_detail_alert),
        )
        .route(
            "/devices/{device_id}/details/app-usage/{id}",
            delete(delete_detail_app_usage),
        )
        .route(
            "/devices/{device_id}/locate",
            post(request_location_command),
        )
        .route(
            "/devices/{device_id}/permissions/{permission}/status",
            post(request_permission_status_command),
        )
        .route(
            "/devices/{device_id}/capabilities/{capability}/request",
            post(request_capability_upload_command),
        )
        .route(
            "/devices/{device_id}/open-app",
            post(open_app_command),
        )
        .route("/devices/{device_id}", delete(delete_device))
        .route("/heartbeat", post(heartbeat))
        .route("/location", post(update_location)) //Connects POST /location URL to update_location function.
        .with_state(state);

    let port = env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let bind_addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();

    println!("server running on http://{bind_addr}");

    axum::serve(listener, app).await.unwrap(); //runs the web server forever
}

/*AndroidManifest.xml
allows location permission

MainActivity.kt
asks permission + reads location

LocationRequest.kt
defines location JSON

ApiService.kt
sends location JSON to POST /location

backend main.rs
receives and stores location

device.rs
returns location from GET /devices */
