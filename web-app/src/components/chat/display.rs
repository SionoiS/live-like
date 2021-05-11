use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::str;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::components::chat::message::{MessageData, UIMessage};
use crate::utils::ipfs::{IpfsService, PubsubSubResponse};

use wasm_bindgen_futures::spawn_local;

use yew::prelude::{html, Component, ComponentLink, Html, Properties, ShouldRender};
use yew::services::ConsoleService;

use cid::Cid;

use linked_data::chat::{Content, SignedMessage, UnsignedMessage};

use reqwest::Error;

use blockies::Ethereum;

pub struct Display {
    link: ComponentLink<Self>,

    ipfs: IpfsService,
    img_gen: Ethereum,

    /// Signed Message Cid Mapped to address, peer id and name
    trusted_identities: HashMap<Cid, ([u8; 20], Content)>,

    /// Peer Id with Unsigned Messages
    msg_buffer: Vec<(String, UnsignedMessage)>,

    next_id: usize,
    chat_messages: VecDeque<MessageData>,

    drop_sig: Rc<AtomicBool>,
}

pub enum Msg {
    PubSub(Result<PubsubSubResponse, std::io::Error>),
    Origin((Cid, Result<SignedMessage, Error>)),
}

#[derive(Properties, Clone)]
pub struct Props {
    pub ipfs: IpfsService,
    pub topic: Rc<str>,
}

impl Component for Display {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        let Props { ipfs, topic } = props;

        let client = ipfs.clone();
        let cb = link.callback(Msg::PubSub);
        let sub_topic = topic.to_string();

        let drop_sig = Rc::from(AtomicBool::new(false));
        let sig = drop_sig.clone();

        spawn_local(async move { client.pubsub_sub(sub_topic, cb, sig).await });

        //https://github.com/ethereum/blockies
        //https://docs.rs/blockies/0.3.0/blockies/struct.Ethereum.html
        let img_gen = Ethereum {
            size: 8,
            scale: 4,
            color: None,
            background_color: None,
            spot_color: None,
        };

        Self {
            link,

            ipfs,
            img_gen,

            trusted_identities: HashMap::with_capacity(100),

            msg_buffer: Vec::with_capacity(10),

            chat_messages: VecDeque::with_capacity(20),
            next_id: 0,

            drop_sig,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::PubSub(result) => self.on_pubsub_update(result),
            Msg::Origin((cid, result)) => self.on_signed_msg(cid, result),
        }
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        false
    }

    fn view(&self) -> Html {
        html! {
        <div class="chat_display">
        {
        for self.chat_messages.iter().map(|cm| html! {
            <UIMessage key=cm.id.to_string() message_data=cm />
        })
        }
        </div>
        }
    }

    fn destroy(&mut self) {
        #[cfg(debug_assertions)]
        ConsoleService::info("Dropping Live Chat");

        self.drop_sig.store(true, Ordering::Relaxed);
    }
}

impl Display {
    /// Callback when GossipSub receive a message.
    fn on_pubsub_update(&mut self, result: Result<PubsubSubResponse, std::io::Error>) -> bool {
        let res = match result {
            Ok(res) => res,
            Err(e) => {
                ConsoleService::error(&format!("{:?}", e));
                return false;
            }
        };

        #[cfg(debug_assertions)]
        ConsoleService::info("PubSub Message Received");

        let PubsubSubResponse { from, data } = res;

        #[cfg(debug_assertions)]
        ConsoleService::info(&format!("Sender => {}", from));

        let msg: UnsignedMessage = match serde_json::from_slice(&data) {
            Ok(msg) => msg,
            Err(e) => {
                ConsoleService::error(&format!("{:?}", e));
                return false;
            }
        };

        #[cfg(debug_assertions)]
        ConsoleService::info(&format!("Message => {}", msg.message));

        if !self.is_allowed(&from, &msg.origin.link) {
            return false;
        }

        match self.trusted_identities.get(&msg.origin.link) {
            Some((addrs, content)) => {
                if content.peer_id == from {
                    let mut data = Vec::new();

                    self.img_gen
                        .create_icon(&mut data, addrs)
                        .expect("Invalid Blocky");

                    let msg_data =
                        MessageData::new(self.next_id, &data, &content.name, &msg.message);

                    self.chat_messages.push_back(msg_data);

                    if self.chat_messages.len() >= 10 {
                        self.chat_messages.pop_front();
                    }

                    self.next_id += 1;

                    return true;
                }
            }
            None => {
                let cb = self.link.callback_once(Msg::Origin);
                let client = self.ipfs.clone();
                let cid = msg.origin.link;

                self.msg_buffer.push((from, msg));

                spawn_local(async move {
                    cb.emit((cid, client.dag_get(cid, Option::<String>::None).await))
                });
            }
        }

        false
    }

    /// Verify identity against white & black lists
    fn is_allowed(&self, _from: &str, _cid: &Cid) -> bool {
        //TODO verify white & black list
        true
        //self.whitelist.whitelist.contains(identity) || !self.blacklist.blacklist.contains(identity)
    }

    /// Callback when IPFS dag get signed message node.
    fn on_signed_msg(&mut self, cid: Cid, response: Result<SignedMessage, Error>) -> bool {
        let sign_msg = match response {
            Ok(m) => m,
            Err(e) => {
                ConsoleService::error(&format!("{:?}", e));
                return false;
            }
        };

        #[cfg(debug_assertions)]
        ConsoleService::info("Signed Message Received");

        let verified = sign_msg.verify();

        #[cfg(debug_assertions)]
        ConsoleService::info(&format!("Verifiable => {}", verified));

        let mut i = self.msg_buffer.len();
        while i != 0 {
            let (peer_id, msg) = &self.msg_buffer[i - 1];

            if cid != msg.origin.link {
                continue;
            }

            if *peer_id == sign_msg.data.peer_id && verified {
                let mut data = Vec::new();

                self.img_gen
                    .create_icon(&mut data, &sign_msg.address)
                    .expect("Invalid Blocky");

                let msg_data =
                    MessageData::new(self.next_id, &data, &sign_msg.data.name, &msg.message);

                self.chat_messages.push_back(msg_data);

                if self.chat_messages.len() >= 10 {
                    self.chat_messages.pop_front();
                }

                self.next_id += 1;
            }

            self.msg_buffer.swap_remove(i - 1);

            i -= 1;
        }

        if verified {
            self.trusted_identities
                .insert(cid, (sign_msg.address, sign_msg.data));
        }

        true
    }
}