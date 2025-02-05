use crate::error::Error;
use crate::model::TestModel;
use crate::response::{
    AnthropicResponse,
    CohereResponse,
    IntoChatResponse,
    OpenAiResponse,
};
use std::fmt;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ApiProvider {
    OpenAi { url: String },
    Cohere,
    Anthropic,

    /// for test
    /// 1. doesn't require api key
    /// 2. needs no network
    Test(TestModel),
}

impl ApiProvider {
    // TODO: why `XXXResponse` -> `Box<dyn IntoChatResponse>` -> `Response`?
    //       why not just `XXXResponse` -> `Response`?
    pub fn parse_chat_response(&self, s: &str) -> Result<Box<dyn IntoChatResponse>, Error> {
        match self {
            ApiProvider::Anthropic => Ok(Box::new(serde_json::from_str::<AnthropicResponse>(s)?)),
            ApiProvider::Cohere => Ok(Box::new(serde_json::from_str::<CohereResponse>(s)?)),
            ApiProvider::OpenAi { .. } => Ok(Box::new(serde_json::from_str::<OpenAiResponse>(s)?)),
            ApiProvider::Test(_) => unreachable!(),
        }
    }

    pub fn parse(s: &str, url: &Option<String>) -> Result<Self, Error> {
        match s.to_ascii_lowercase().replace(" ", "").replace("-", "").as_str() {
            "openai" => match url {
                Some(url) => Ok(ApiProvider::OpenAi { url: url.to_string() }),
                None => Ok(ApiProvider::OpenAi { url: String::from("https://api.openai.com/v1/chat/completions") }),
            },
            "cohere" => Ok(ApiProvider::Cohere),
            "anthropic" => Ok(ApiProvider::Anthropic),
            _ => Err(Error::InvalidApiProvider(s.to_string())),
        }
    }

    pub fn get_api_url(&self) -> &str {
        match self {
            ApiProvider::Anthropic => "https://api.anthropic.com/v1/messages",
            ApiProvider::Cohere => "https://api.cohere.com/v2/chat",
            ApiProvider::OpenAi { url } => url,
            ApiProvider::Test(_) => "",
        }
    }
}

impl fmt::Display for ApiProvider {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            fmt,
            "{}",
            match self {
                ApiProvider::OpenAi { .. } => "openai",
                ApiProvider::Cohere => "cohere",
                ApiProvider::Anthropic => "anthropic",
                ApiProvider::Test(_) => "test",
            },
        )
    }
}
