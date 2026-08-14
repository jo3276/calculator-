/*device.rs is used to repersent device data from database including phone info.
  location info cmd status and last activity this data is showed in the backend*/


use chrono::NaiveDateTime; //import timestamp type
use serde::Serialize;  // this will import JSON serialization
use sqlx::FromRow; //import sql row mapping

#[derive(Serialize, Clone, FromRow)] /*convert this to json and cloned and fill directly to
                                      sql query rows */
pub struct Device {
    pub device_id: String, //required text
    pub manufacturer: Option<String>, //nullable text from database
    pub model: Option<String>,
    pub android_version: Option<String>,
    pub latitude: Option<f64>,  //Device JSON response can include location.
    pub longitude: Option<f64>, /*Why Option<f64>:Old devices may not have location yet
                                so value can be null */
    pub location_accuracy_meters: Option<f64>, // store how accurate gps location is 
    pub location_time: Option<NaiveDateTime>, //store when gps location captured
    pub last_command_status: Option<String>,  //store the cmd status like location uploded no gps fix 
    pub last_command_status_time: Option<NaiveDateTime>, //store which time command status updated
    pub last_seen: Option<NaiveDateTime>,  //store when device contacted backend last time
    pub command_ready: bool, //true when device register FCM token
    pub online: Option<bool>, //calculated from last seen
}
