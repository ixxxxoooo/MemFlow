// @author ygw
//! 向量化后端的统一入口:按设置在本地 Ollama 与 OpenAI 兼容云端(`/embeddings`)之间切换。

use crate::error::Result;
use crate::ollama::OllamaClient;
use crate::openai::OpenAiClient;

/// 向量化客户端
pub enum EmbedClient {
    /// 本地 Ollama
    Ollama {
        client: OllamaClient,
        model: String,
        keep_alive: String,
    },
    /// OpenAI 兼容云端
    Cloud { client: OpenAiClient, model: String },
}

impl EmbedClient {
    /// 是否为云端
    pub fn is_cloud(&self) -> bool {
        matches!(self, EmbedClient::Cloud { .. })
    }

    /// 批量向量化,结果已做 L2 归一化并校验合法性
    ///
    /// 参数: inputs 文本列表
    /// 返回: 与输入等长的向量列表
    pub async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        match self {
            EmbedClient::Ollama {
                client,
                model,
                keep_alive,
            } => client.embed(model, inputs, keep_alive).await,
            EmbedClient::Cloud { client, model } => client.embed(model, inputs).await,
        }
    }
}
