use std::collections::HashMap;

use crate::IPLDLink;

use serde::{Deserialize, Serialize};

/// Links all variants, allowing selection of video quality. Also link to the previous video node.
#[derive(Serialize, Deserialize, Debug)]
pub struct VideoNode {
    // <StreamHash>/time/hour/0/minute/36/second/12/video/quality/1080p60/..
    #[serde(rename = "quality")]
    pub qualities: HashMap<String, IPLDLink>,

    // <StreamHash>/time/hour/0/minute/36/second/12/video/setup/..
    #[serde(rename = "setup")]
    pub setup: IPLDLink,

    // <StreamHash>/time/hour/0/minute/36/second/12/video/previous/..
    #[serde(rename = "previous")]
    pub previous: Option<IPLDLink>,
}

/// Codecs, qualities & initialization segments from lowest to highest quality.
#[derive(Serialize, Deserialize, Debug)]
pub struct SetupNode {
    // <StreamHash>/time/hour/0/minute/36/second/12/video/setup/quality
    #[serde(rename = "quality")]
    pub qualities: Vec<String>,

    // <StreamHash>/time/hour/0/minute/36/second/12/video/setup/codec
    #[serde(rename = "codec")]
    pub codecs: Vec<String>,

    // <StreamHash>/time/hour/0/minute/36/second/12/video/setup/initseg/0/..
    #[serde(rename = "initseg")]
    pub initialization_segments: Vec<IPLDLink>,
}