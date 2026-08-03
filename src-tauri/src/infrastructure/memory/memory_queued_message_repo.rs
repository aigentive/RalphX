use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;

use crate::domain::repositories::QueuedMessageRepository;
use crate::domain::services::{QueueKey, QueuedMessage};
use crate::error::AppResult;

pub struct MemoryQueuedMessageRepository {
    queues: RwLock<HashMap<QueueKey, Vec<QueuedMessage>>>,
}

impl MemoryQueuedMessageRepository {
    pub fn new() -> Self {
        Self {
            queues: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryQueuedMessageRepository {
    fn default() -> Self {
        Self::new()
    }
}

fn remove_message_id(queues: &mut HashMap<QueueKey, Vec<QueuedMessage>>, message_id: &str) -> bool {
    let mut removed = false;
    queues.retain(|_, queue| {
        let before = queue.len();
        queue.retain(|message| message.id != message_id);
        removed |= queue.len() != before;
        !queue.is_empty()
    });
    removed
}

#[async_trait]
impl QueuedMessageRepository for MemoryQueuedMessageRepository {
    async fn enqueue_back(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
        let mut queues = self.queues.write().unwrap();
        remove_message_id(&mut queues, &message.id);
        queues.entry(key.clone()).or_default().push(message.clone());
        Ok(())
    }

    async fn enqueue_front(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
        let mut queues = self.queues.write().unwrap();
        remove_message_id(&mut queues, &message.id);
        queues
            .entry(key.clone())
            .or_default()
            .insert(0, message.clone());
        Ok(())
    }

    async fn list(&self, key: &QueueKey) -> AppResult<Vec<QueuedMessage>> {
        let queues = self.queues.read().unwrap();
        Ok(queues.get(key).cloned().unwrap_or_default())
    }

    async fn list_keys(&self) -> AppResult<Vec<QueueKey>> {
        let queues = self.queues.read().unwrap();
        Ok(queues
            .iter()
            .filter_map(|(key, queue)| {
                if queue.is_empty() {
                    None
                } else {
                    Some(key.clone())
                }
            })
            .collect())
    }

    async fn delete(&self, key: &QueueKey, message_id: &str) -> AppResult<bool> {
        let mut queues = self.queues.write().unwrap();
        let Some(queue) = queues.get_mut(key) else {
            return Ok(false);
        };
        let Some(index) = queue.iter().position(|message| message.id == message_id) else {
            return Ok(false);
        };
        queue.remove(index);
        if queue.is_empty() {
            queues.remove(key);
        }
        Ok(true)
    }

    async fn delete_by_id(&self, message_id: &str) -> AppResult<bool> {
        let mut queues = self.queues.write().unwrap();
        Ok(remove_message_id(&mut queues, message_id))
    }

    async fn clear(&self, key: &QueueKey) -> AppResult<()> {
        let mut queues = self.queues.write().unwrap();
        queues.remove(key);
        Ok(())
    }

    async fn pop_front(&self, key: &QueueKey) -> AppResult<Option<QueuedMessage>> {
        let mut queues = self.queues.write().unwrap();
        let Some(queue) = queues.get_mut(key) else {
            return Ok(None);
        };
        if queue.is_empty() {
            return Ok(None);
        }
        let message = queue.remove(0);
        if queue.is_empty() {
            queues.remove(key);
        }
        Ok(Some(message))
    }

    async fn remove_stale(
        &self,
        key: &QueueKey,
        threshold_secs: u64,
    ) -> AppResult<Vec<QueuedMessage>> {
        let mut queues = self.queues.write().unwrap();
        let Some(queue) = queues.get_mut(key) else {
            return Ok(Vec::new());
        };

        let now = chrono::Utc::now();
        let mut dropped = Vec::new();
        queue.retain(|message| {
            let is_stale = chrono::DateTime::parse_from_rfc3339(&message.created_at)
                .map(|timestamp| {
                    let age = now.signed_duration_since(timestamp.with_timezone(&chrono::Utc));
                    age.num_seconds() > threshold_secs as i64
                })
                .unwrap_or(false);
            let is_stale_hidden_recovery = is_stale && message.is_hidden_recovery();
            if is_stale_hidden_recovery {
                dropped.push(message.clone());
            }
            !is_stale_hidden_recovery
        });
        if queue.is_empty() {
            queues.remove(key);
        }
        Ok(dropped)
    }
}

#[cfg(test)]
#[path = "memory_queued_message_repo_tests.rs"]
mod tests;
