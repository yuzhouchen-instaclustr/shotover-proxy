use crate::config::topology::TopicHolder;
use crate::error::ChainResponse;
use crate::transforms::chain::{BufferedChain, TransformChain};
use crate::transforms::{
    build_chain_from_config, Transform, Transforms, TransformsConfig, TransformsFromConfig, Wrapper,
};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct ConnectionBalanceAndPoolConfig {
    pub name: String,
    pub parallelism: usize,
    pub chain: Vec<TransformsConfig>,
}

#[async_trait]
impl TransformsFromConfig for ConnectionBalanceAndPoolConfig {
    async fn get_source(&self, topics: &TopicHolder) -> Result<Transforms> {
        let chain = build_chain_from_config(self.name.clone(), &self.chain, &topics).await?;

        Ok(Transforms::PoolConnections(ConnectionBalanceAndPool {
            name: "PoolConnections",
            active_connection: None,
            parallelism: self.parallelism,
            other_connections: Arc::new(Mutex::new(Vec::with_capacity(self.parallelism))),
            chain_to_clone: chain,
        }))
    }
}

#[derive(Debug)]
pub struct ConnectionBalanceAndPool {
    pub name: &'static str,
    pub active_connection: Option<BufferedChain>,
    pub parallelism: usize,
    pub other_connections: Arc<Mutex<Vec<BufferedChain>>>,
    pub chain_to_clone: TransformChain,
}

impl Clone for ConnectionBalanceAndPool {
    fn clone(&self) -> Self {
        ConnectionBalanceAndPool {
            name: self.name,
            active_connection: None,
            parallelism: self.parallelism,
            other_connections: self.other_connections.clone(),
            chain_to_clone: self.chain_to_clone.clone(),
        }
    }
}

#[async_trait]
impl Transform for ConnectionBalanceAndPool {
    async fn transform<'a>(&'a mut self, message_wrapper: Wrapper<'a>) -> ChainResponse {
        if self.active_connection.is_none() {
            let mut guard = self.other_connections.lock().await;
            if guard.len() < self.parallelism {
                let chain = self.chain_to_clone.clone().build_buffered_chain(5);
                self.active_connection.replace(chain.clone());
                guard.push(chain);
            } else {
                //take the first available existing change and grab its reference
                let top = guard.remove(0);
                self.active_connection.replace(top.clone());
                // put the chain at the back of the list
                guard.push(top);
            }
        }
        if let Some(chain) = &mut self.active_connection {
            return chain
                .process_request(
                    message_wrapper,
                    "Connection Balance and Pooler".to_string(),
                    None,
                )
                .await;
        }
        unreachable!()
    }

    fn get_name(&self) -> &'static str {
        self.name
    }
}

#[cfg(test)]
mod test {
    use crate::message::Messages;
    use crate::transforms::chain::TransformChain;
    use crate::transforms::load_balance::ConnectionBalanceAndPool;
    use crate::transforms::test_transforms::ReturnerTransform;
    use crate::transforms::{Transforms, Wrapper};
    use anyhow::Result;
    use std::sync::Arc;

    #[tokio::test(flavor = "multi_thread")]
    pub async fn test_balance() -> Result<()> {
        let transform = Transforms::PoolConnections(ConnectionBalanceAndPool {
            name: "",
            active_connection: None,
            parallelism: 3,
            other_connections: Arc::new(Default::default()),
            chain_to_clone: TransformChain::new(
                vec![Transforms::RepeatMessage(Box::new(ReturnerTransform {
                    message: Messages::new(),
                    ok: true,
                }))],
                "child_test".to_string(),
            ),
        });

        let mut chain = TransformChain::new(vec![transform], "test".to_string());

        for _ in 0..90 {
            let r = chain
                .clone()
                .process_request(Wrapper::new(Messages::new()), "test_client".to_string())
                .await;
            assert_eq!(r.is_ok(), true);
        }

        match chain.chain.remove(0) {
            Transforms::PoolConnections(p) => {
                let guard = p.other_connections.lock().await;
                assert_eq!(guard.len(), 3);
                for bc in guard.iter() {
                    let guard = bc.count.lock().await;
                    assert_eq!(*guard, 30);
                }
            }
            _ => panic!("whoops"),
        }

        Ok(())
    }
}