use std::borrow::Cow;
use std::convert::TryFrom;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::utils::local_storage::LocalStorage;

use futures::join;
use futures_util::{AsyncBufReadExt, StreamExt, TryStreamExt};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use yew::services::ConsoleService;
use yew::Callback;

use cid::multibase::Base;
use cid::Cid;

use reqwest::multipart::Form;
use reqwest::{Client, Url};

const DEFAULT_URI: &str = "http://localhost:5001/api/v0/";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone)]
pub struct IpfsService {
    client: Client,
    base_url: Rc<Url>,
}

impl IpfsService {
    pub fn new(storage: &LocalStorage) -> Self {
        let result = match storage.get_local_ipfs_addrs() {
            Some(addrs) => Url::parse(&addrs),
            None => {
                storage.set_local_ipfs_addrs(DEFAULT_URI);

                Url::parse(DEFAULT_URI)
            }
        };

        let url = match result {
            Ok(url) => url,
            Err(e) => {
                ConsoleService::error(&format!("{:#?}", e));
                std::process::abort();
            }
        };

        let client = Client::new();
        let base_url = Rc::from(url);

        Self { client, base_url }
    }

    /// Download content from block with this CID.
    pub async fn cid_cat(&self, cid: Cid) -> Result<Vec<u8>> {
        let url = self.base_url.join("cat")?;

        let bytes = self
            .client
            .post(url)
            .query(&[("arg", &cid.to_string())])
            .send()
            .await?
            .bytes()
            .await?;

        Ok(bytes.to_vec())
    }

    /// Download content simultaneously from 2 paths.
    pub async fn double_path_cat<U>(
        &self,
        audio_path: U,
        video_path: U,
    ) -> Result<(Vec<u8>, Vec<u8>)>
    where
        U: Into<Cow<'static, str>>,
    {
        let url = self.base_url.join("cat")?;

        let (audio_res, video_res) = join!(
            self.client
                .post(url.clone())
                .query(&[("arg", &audio_path.into())])
                .send(),
            self.client
                .post(url)
                .query(&[("arg", &video_path.into())])
                .send()
        );

        let audio_data = audio_res?;
        let video_data = video_res?;

        let (audio_result, video_result) = join!(audio_data.bytes(), video_data.bytes(),);

        let audio_data = audio_result?;
        let video_data = video_result?;

        Ok((audio_data.to_vec(), video_data.to_vec()))
    }

    /// Serialize then add dag node to IPFS. Return a CID.
    pub async fn dag_put<T>(&self, node: &T) -> Result<Cid>
    where
        T: ?Sized + Serialize,
    {
        #[cfg(debug_assertions)]
        ConsoleService::info(&format!(
            "Serde: Serialize => {}",
            serde_json::to_string_pretty(node).unwrap()
        ));

        let data = serde_json::to_string(node)?;

        //Reqwest was hacked to properly format multipart request with text ONLY
        let form = Form::new().text("object data", data);

        let url = self.base_url.join("dag/put")?;

        let response: DagPutResponse = self
            .client
            .post(url)
            .multipart(form)
            .send()
            .await?
            .json()
            .await?;

        let cid = Cid::try_from(response.cid.cid_string)?;

        #[cfg(debug_assertions)]
        ConsoleService::info(&format!("IPFS: dag put => {}", &cid));

        Ok(cid)
    }

    /// Deserialize dag node from IPFS path. Return dag node.
    pub async fn dag_get<U, T>(&self, cid: Cid, path: Option<U>) -> Result<T>
    where
        U: Into<Cow<'static, str>>,
        T: ?Sized + DeserializeOwned,
    {
        let mut origin = cid.to_string();

        if let Some(path) = path {
            origin.push_str(&path.into());
        }

        #[cfg(debug_assertions)]
        ConsoleService::info(&format!("IPFS: dag get => {}", origin));

        let url = self.base_url.join("dag/get")?;

        let res = self
            .client
            .post(url)
            .query(&[("arg", &origin)])
            .send()
            .await?;

        let node = res.json::<T>().await?;

        Ok(node)
    }

    /// Resolve IPNS link then dag get. Return CID and Node.
    pub async fn resolve_and_dag_get<U, T>(&self, ipns: U) -> Result<(Cid, T)>
    where
        U: Into<Cow<'static, str>>,
        T: ?Sized + DeserializeOwned,
    {
        let url = self.base_url.join("name/resolve")?;

        let res: NameResolveResponse = self
            .client
            .post(url)
            .query(&[("arg", &ipns.into())])
            .send()
            .await?
            .json()
            .await?;

        let cid = Cid::try_from(res.path)?;

        #[cfg(debug_assertions)]
        ConsoleService::info(&format!("IPFS: name resolve => {}", cid.to_string()));

        let node = self.dag_get(cid, Option::<&str>::None).await?;

        Ok((cid, node))
    }

    /// Subscribe to a topic then deserialize output.
    pub async fn pubsub_sub<U>(
        &self,
        topic: U,
        cb: Callback<Result<(String, Vec<u8>)>>,
        drop_sig: Rc<AtomicBool>,
    ) where
        U: Into<Cow<'static, str>>,
    {
        let url = match self.base_url.join("pubsub/sub") {
            Ok(url) => url,
            Err(e) => {
                cb.emit(Err(e.into()));
                return;
            }
        };

        let result = self
            .client
            .post(url)
            .query(&[("arg", &topic.into())])
            .send()
            .await;

        let stream = match result {
            Ok(res) => res.bytes_stream(),
            Err(e) => {
                cb.emit(Err(e.into()));
                return;
            }
        };

        let mut line_stream = stream.err_into().into_async_read().lines();

        while let Some(result) = line_stream.next().await {
            if drop_sig.load(Ordering::Relaxed) {
                // Hacky way I found to close the stream
                return;
            }

            let line = match result {
                Ok(line) => line,
                Err(e) => {
                    cb.emit(Err(e.into()));
                    return;
                }
            };

            let response = match serde_json::from_str::<PubsubSubResponse>(&line) {
                Ok(node) => node,
                Err(e) => {
                    cb.emit(Err(e.into()));
                    return;
                }
            };

            let PubsubSubResponse { from, data } = response;

            let from = match Base::decode(&Base::Base64Pad, from) {
                Ok(from) => from,
                Err(e) => {
                    cb.emit(Err(e.into()));
                    return;
                }
            };

            //This is the most common encoding for PeerIds
            let from = Base::encode(&Base::Base58Btc, from);

            let data = match Base::decode(&Base::Base64Pad, data) {
                Ok(from) => from,
                Err(e) => {
                    cb.emit(Err(e.into()));
                    return;
                }
            };

            cb.emit(Ok((from, data)))
        }
    }

    pub async fn pubsub_pub<U>(&self, topic: U, msg: U) -> Result<()>
    where
        U: Into<Cow<'static, str>>,
    {
        let url = self.base_url.join("pubsub/pub")?;

        self.client
            .post(url)
            .query(&[("arg", &topic.into()), ("arg", &msg.into())])
            .send()
            .await?;

        Ok(())
    }

    pub async fn ipfs_node_id(&self) -> Result<String> {
        let url = self.base_url.join("id")?;

        let response = self
            .client
            .post(url)
            .send()
            .await?
            .json::<IdResponse>()
            .await?;

        Ok(response.id)
    }
}

#[derive(Deserialize)]
struct PubsubSubResponse {
    pub from: String,
    pub data: String,
}

#[derive(Deserialize)]
struct DagPutResponse {
    #[serde(rename = "Cid")]
    pub cid: CidString,
}

#[derive(Deserialize)]
struct CidString {
    #[serde(rename = "/")]
    pub cid_string: String,
}

#[derive(Deserialize)]
struct NameResolveResponse {
    #[serde(rename = "Path")]
    pub path: String,
}

#[derive(Deserialize)]
struct IdResponse {
    #[serde(rename = "ID")]
    pub id: String,
}
