use std::collections::HashMap;
use crate::transforms::chain::{TransformChain, Wrapper, Transform, ChainResponse, RequestError};
use futures::future::join_all;

use async_trait::async_trait;
use crate::message::{QueryResponse, Message};
use futures::stream::FuturesUnordered;
use tokio::stream::StreamExt;
use serde::{Serialize, Deserialize};
use crate::transforms::{Transforms, TransformsConfig, build_chain_from_config, TransformsFromConfig};
use crate::runtimes::rhai::RhaiEnvironment;
use crate::config::ConfigError;
use crate::config::topology::TopicHolder;


#[derive(Clone)]
pub struct Scatter {
    name: &'static str,
    route_map: HashMap<String, TransformChain>,
    function_env: RhaiEnvironment,
    reduce_scatter_results: bool
}


#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct ScatterConfig {
    #[serde(rename = "config_values")]
    pub route_map: HashMap<String, Vec<TransformsConfig>>,
    pub rhai_script: String,
    reduce_scatter_results: bool
}

#[async_trait]
impl TransformsFromConfig for ScatterConfig {
    async fn get_source(&self, topics: &TopicHolder) -> Result<Transforms, ConfigError> {
        let mut temp: HashMap<String, TransformChain> = HashMap::new();
        for (key, value) in self.route_map.clone() {
            temp.insert(key.clone(), build_chain_from_config(key, &value, topics).await?);
        }
        Ok(Transforms::Scatter(Scatter{
            name: "scatter",
            route_map: temp,
            function_env: RhaiEnvironment::new(&self.rhai_script)?,
            reduce_scatter_results: self.reduce_scatter_results
        }))
    }
}

#[async_trait]
impl Transform for Scatter {
    async fn transform(&self, mut qd: Wrapper, t: &TransformChain) -> ChainResponse {
        let routes: Vec<String> = self.route_map.keys().map(|x| x).cloned().collect();
        let chosen_route = self.function_env.call_scatter_route(qd.clone(), routes)?;
        if chosen_route.len() == 1 {
            return self.route_map.get(chosen_route.get(0).unwrap().as_str()).unwrap().process_request(qd).await;
        } else if chosen_route.len() == 0 {
            return ChainResponse::Err(RequestError{})
        } else {
            let mut fu = FuturesUnordered::new();
            for ref route in &chosen_route {
                let chain = self.route_map.get(route.as_str()).unwrap();
                let mut wrapper = qd.clone();
                wrapper.reset();
                fu.push(chain.process_request(wrapper));
            }
            // TODO I feel like there should be some streamext function that does this for me
            return if self.reduce_scatter_results {
                self.function_env.call_scatter_handle_func(fu.collect().await, chosen_route)
            } else {
                while let Some(r) = fu.next().await {
                    if let Err(e) = r {
                        return ChainResponse::Err(RequestError{})
                    }
                }
                ChainResponse::Ok(Message::Response(QueryResponse::empty()))
            }
        }
    }

    fn get_name(&self) -> &'static str {
        self.name
    }
}