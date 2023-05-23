use crate::{imp::{core::*, prelude::*, utils::*}};

#[skip_serializing_none]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FetchArgs {
}

#[skip_serializing_none]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FormType {
    name: String,
    value: String
}

impl From<(String, String)> for FormType {
    fn from((name, value): (String, String)) -> Self {
        Self { name, value }
    }
}

#[derive(Debug)]
pub(crate) enum DataType {
    Data(String),
    Form(Map<String, Value>),
    Multipart(Map<String, Value>)
}

#[derive(Debug)]
pub(crate) struct ApiRequestContext {
    channel: ChannelOwner,
    var: Mutex<Variable>
}

#[derive(Debug, Default)]
pub(crate) struct Variable {
}

impl ApiRequestContext {
    pub(crate) fn try_new(channel: ChannelOwner) -> Result<Self, Error> {
        let Initializer { } = serde_json::from_value(channel.initializer.clone())?;
        Ok(Self {
            channel,
            var: Mutex::default()
        })
    }

    pub(crate) async fn dispose(&self) -> ArcResult<()> {
        let _ = send_message!(self, "dispose", Map::new());
        Ok(())
    }

    pub(crate) async fn storage_state(&self) -> ArcResult<StorageState> {
        let v = send_message!(self, "storageState", Map::new());
        let s = serde_json::from_value((*v).clone()).map_err(Error::Serde)?;
        Ok(s)
    }

    pub(crate) async fn fetch(
        &self,
        url: &str,
        method: &str,
        headers: Option<Map<String, Value>>,
        data: Option<DataType>,
        params: Option<Map<String, Value>>,
        timeout: Option<f32>,
        fail_on_status_code: Option<bool>,
        ignore_http_errors: Option<bool>,
        max_redirects: Option<u32>, 
    ) -> ArcResult<ApiResponse> {
        let mut post_data_buffer = None;
        let mut json_data = None;
        let mut form_data = None;
        let mut multipart_data = None;
        if let Some(data) = data {
            match data {
                DataType::Data(s) => {
                    if is_json_content_type(&headers) {
                        json_data = Some(s);
                    } else {
                        post_data_buffer = Some(s);
                    }
                },
                DataType::Form(m) => {
                    form_data = Some(m);
                },
                DataType::Multipart(m) => {
                    multipart_data = Some(m);
                }
            }
        }
        Ok(ApiResponse {  })       
    }

}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Initializer {
}

fn is_json_content_type(headers: &Option<Map<String, Value>>) -> bool {
    let headers = match headers {
        Some(headers) => headers,
        None => return false
    };
    if let Some(value) = headers.get("content-type") {
        return value.as_str().starts_with("application/json");
    }
    return false;
}