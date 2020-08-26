use crate::error::{ChainResponse, RequestError};
use crate::message::Messages;
use crate::transforms::Transforms;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use bytes::Bytes;
use evmap::ReadHandleFactory;
use metrics::{counter, timing};
use mlua::{Lua, UserData};
use serde::export::Formatter;
use std::fmt::Display;
use std::num::Wrapping;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::time::Instant;

#[derive(Debug, Clone)]
struct QueryData {
    query: String,
}

#[derive(Debug, Clone)]
pub struct Wrapper {
    pub message: Messages,
    pub next_transform: usize,
    pub modified: bool,
    pub clock: Wrapping<u32>,
}

impl UserData for Wrapper {}

impl Display for Wrapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        f.write_fmt(format_args!("{:#?}", self.message))
    }
}

impl Wrapper {
    pub fn swap_message(&mut self, mut m: Messages) {
        std::mem::swap(&mut self.message, &mut m);
    }

    pub fn new(m: Messages) -> Self {
        Wrapper {
            message: m,
            next_transform: 0,
            modified: false,
            clock: Wrapping(0),
        }
    }

    pub fn new_with_next_transform(m: Messages, next_transform: usize) -> Self {
        Wrapper {
            message: m,
            next_transform,
            modified: false,
            clock: Wrapping(0),
        }
    }

    pub fn new_with_rnd(m: Messages, clock: Wrapping<u32>) -> Self {
        Wrapper {
            message: m,
            next_transform: 0,
            modified: false,
            clock,
        }
    }

    pub fn reset(&mut self) {
        self.next_transform = 0;
    }
}

#[derive(Debug)]
struct ResponseData {
    response: Messages,
}

//TODO change Transform to maintain the InnerChain internally so we don't have to expose this
pub type InnerChain = Vec<Transforms>;

//TODO explore running the transform chain on a LocalSet for better locality to a given OS thread
//Will also mean we can have `!Send` types  in our transform chain

#[async_trait]
pub trait Transform: Send {
    async fn transform(&self, mut qd: Wrapper, t: &TransformChain) -> ChainResponse;

    fn get_name(&self) -> &'static str;

    async fn prep_transform_chain(&mut self, _t: &mut TransformChain) -> Result<()> {
        Ok(())
    }

    async fn instrument_transform(&self, qd: Wrapper, t: &TransformChain) -> ChainResponse {
        let start = Instant::now();
        let result = self.transform(qd, t).await;
        let end = Instant::now();
        counter!("shotover_transform_total", 1, "transform" => self.get_name());
        if let Err(_) = &result {
            counter!("shotover_transform_failures", 1, "transform" => self.get_name())
        }
        timing!("shotover_transform_latency", start, end, "transform" => self.get_name());
        result
    }

    async fn call_next_transform(
        &self,
        mut qd: Wrapper,
        transforms: &TransformChain,
    ) -> ChainResponse {
        let next = qd.next_transform;
        qd.next_transform += 1;
        match transforms.chain.get(next) {
            Some(t) => t.instrument_transform(qd, transforms).await,
            None => Err(anyhow!(RequestError::ChainProcessingError(
                "No more transforms left in the chain".to_string()
            ))),
        }
    }
}

// TODO: Remove thread safe requirements for transforms
// Currently the way this chain struct is built, requires that all Transforms are thread safe and
// implement with Sync

// #[derive(Debug)]
pub struct TransformChain {
    name: String,
    pub chain: InnerChain,
    global_map: Option<ReadHandleFactory<String, Bytes>>,
    global_updater: Option<Sender<(String, Bytes)>>,
    pub chain_local_map: Option<ReadHandleFactory<String, Bytes>>,
    pub chain_local_map_updater: Option<Sender<(String, Bytes)>>,
    pub lua_runtime: mlua::Lua,
}

impl Clone for TransformChain {
    fn clone(&self) -> Self {
        TransformChain::new(
            self.chain.clone(),
            self.name.clone(),
            self.global_map.as_ref().unwrap().clone(),
            self.global_updater.as_ref().unwrap().clone(),
        )
    }
}

unsafe impl Send for TransformChain {}
unsafe impl Sync for TransformChain {}

impl TransformChain {
    pub fn new_no_shared_state(transform_list: Vec<Transforms>, name: String) -> Self {
        TransformChain {
            name,
            chain: transform_list,
            global_map: None,
            global_updater: None,
            chain_local_map: None,
            chain_local_map_updater: None,
            lua_runtime: Lua::new(),
        }
    }

    pub fn new(
        transform_list: Vec<Transforms>,
        name: String,
        global_map_handle: ReadHandleFactory<String, Bytes>,
        global_updater: Sender<(String, Bytes)>,
    ) -> Self {
        let (local_tx, mut local_rx): (Sender<(String, Bytes)>, Receiver<(String, Bytes)>) =
            channel::<(String, Bytes)>(1);
        let (rh, mut wh) = evmap::new::<String, Bytes>();
        wh.refresh();

        let _ = tokio::spawn(async move {
            // this loop will exit and the task will complete when all channel tx handles are dropped
            while let Some((k, v)) = local_rx.recv().await {
                wh.insert(k, v);
                wh.refresh();
            }
        });

        TransformChain {
            name,
            chain: transform_list,
            global_map: Some(global_map_handle),
            global_updater: Some(global_updater),
            chain_local_map: Some(rh.factory()),
            chain_local_map_updater: Some(local_tx),
            lua_runtime: Lua::new(),
        }
    }

    pub async fn process_request(
        &self,
        mut wrapper: Wrapper,
        client_details: String,
    ) -> ChainResponse {
        let start = Instant::now();
        let result = match self.chain.get(wrapper.next_transform) {
            Some(t) => {
                wrapper.next_transform += 1;
                t.instrument_transform(wrapper, &self).await
            }
            None => {
                return Err(anyhow!(RequestError::ChainProcessingError(
                    "No more transforms left in the chain".to_string()
                )));
            }
        };
        let end = Instant::now();
        counter!("shotover_chain_total", 1, "chain" => self.name.clone());
        if let Err(_) = &result {
            counter!("shotover_chain_failures", 1, "chain" => self.name.clone())
        }
        timing!("shotover_chain_latency", start, end, "chain" => self.name.clone(), "client_details" => client_details);
        result
    }
}
