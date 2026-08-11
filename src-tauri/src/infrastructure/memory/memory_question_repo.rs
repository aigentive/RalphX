use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;

use crate::domain::entities::question_request::{PendingQuestionInfo, QuestionAnswer};
use crate::domain::repositories::QuestionRepository;
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryQuestionStatus {
    Pending,
    WaitExpired,
    Resolved,
}

struct MemoryQuestionRecord {
    info: PendingQuestionInfo,
    answer: Option<QuestionAnswer>,
    status: MemoryQuestionStatus,
}

pub struct MemoryQuestionRepository {
    questions: RwLock<HashMap<String, MemoryQuestionRecord>>,
}

impl MemoryQuestionRepository {
    pub fn new() -> Self {
        Self {
            questions: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryQuestionRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl QuestionRepository for MemoryQuestionRepository {
    async fn create_pending(&self, info: &PendingQuestionInfo) -> AppResult<()> {
        let mut questions = self.questions.write().unwrap();
        questions.insert(
            info.request_id.clone(),
            MemoryQuestionRecord {
                info: info.clone(),
                answer: None,
                status: MemoryQuestionStatus::Pending,
            },
        );
        Ok(())
    }

    async fn resolve(&self, request_id: &str, answer: &QuestionAnswer) -> AppResult<bool> {
        let mut questions = self.questions.write().unwrap();
        if let Some(entry) = questions.get_mut(request_id) {
            if entry.status == MemoryQuestionStatus::Resolved {
                return Ok(false);
            }
            entry.answer = Some(answer.clone());
            entry.status = MemoryQuestionStatus::Resolved;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn get_pending(&self) -> AppResult<Vec<PendingQuestionInfo>> {
        let questions = self.questions.read().unwrap();
        Ok(questions
            .values()
            .filter(|entry| entry.status != MemoryQuestionStatus::Resolved)
            .map(|entry| entry.info.clone())
            .collect())
    }

    async fn get_by_request_id(&self, request_id: &str) -> AppResult<Option<PendingQuestionInfo>> {
        let questions = self.questions.read().unwrap();
        Ok(questions.get(request_id).map(|entry| entry.info.clone()))
    }

    async fn expire_all_pending(&self) -> AppResult<u64> {
        let mut questions = self.questions.write().unwrap();
        let mut count = 0;
        for entry in questions.values_mut() {
            if entry.status == MemoryQuestionStatus::Pending {
                entry.status = MemoryQuestionStatus::WaitExpired;
                count += 1;
            }
        }
        Ok(count)
    }

    async fn expire_by_request_id(&self, request_id: &str) -> AppResult<()> {
        let mut questions = self.questions.write().unwrap();
        if let Some(entry) = questions.get_mut(request_id) {
            if entry.status == MemoryQuestionStatus::Pending {
                entry.status = MemoryQuestionStatus::WaitExpired;
            }
        }
        Ok(())
    }

    async fn remove(&self, request_id: &str) -> AppResult<bool> {
        let mut questions = self.questions.write().unwrap();
        Ok(questions.remove(request_id).is_some())
    }

    async fn get_resolved_answer(&self, request_id: &str) -> AppResult<Option<QuestionAnswer>> {
        let questions = self.questions.read().unwrap();
        Ok(questions
            .get(request_id)
            .and_then(|entry| entry.answer.clone()))
    }
}

#[cfg(test)]
#[path = "memory_question_repo_tests.rs"]
mod tests;
