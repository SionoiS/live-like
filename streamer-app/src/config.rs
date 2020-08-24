use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub streamer_peer_id: String,
    pub gossipsub_topic: String,
    pub streamer_app: StreamerApp,
    //pub variants: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StreamerApp {
    pub socket_addr: String,
    pub ffmpeg: Option<Ffmpeg>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Ffmpeg {
    pub socket_addr: String,
}
