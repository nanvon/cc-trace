use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub name: &'static str,
    pub version: &'static str,
    pub platform: &'static str,
}
