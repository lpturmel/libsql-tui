use dashmap::DashMap;
use futures::{channel::oneshot, stream::SplitSink, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    fmt::Display,
    sync::{atomic::AtomicI32, Arc},
    time::Instant,
};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async_tls_with_config, MaybeTlsStream, WebSocketStream};
use tungstenite::{
    client::IntoClientRequest,
    http::{header::SEC_WEBSOCKET_PROTOCOL, HeaderValue},
    protocol::WebSocketConfig,
    Message,
};

const PING_REQ_ID: i32 = -1;
const HELLO_REQ_ID: i32 = 1;

pub struct LibSqlClient {
    writer: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    request_id: AtomicI32,
    pending: Arc<DashMap<i32, oneshot::Sender<ResponseType>>>,
}

impl LibSqlClient {
    pub async fn connect(url: &str, jwt: &str) -> color_eyre::Result<Self> {
        #![allow(unused_mut)]
        let mut request = url.into_client_request()?;
        request.headers_mut().append(
            SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_str("hrana3").unwrap(),
        );
        let config = Some(WebSocketConfig::default());
        let (ws_stream, _) = connect_async_tls_with_config(request, config, false, None).await?;
        let (writer, read) = ws_stream.split();
        let mut client = LibSqlClient {
            writer,
            request_id: AtomicI32::new(1),
            pending: Arc::new(DashMap::new()),
        };
        client.spawn_read_loop(read);
        client.send_hello(jwt).await?;
        Ok(client)
    }

    fn spawn_read_loop(
        &self,
        mut read: futures::stream::SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    ) {
        let pending_responses = self.pending.clone();
        tokio::spawn(async move {
            while let Some(msg_result) = read.next().await {
                match msg_result {
                    Ok(tungstenite::Message::Text(text)) => {
                        if let Err(e) = serde_json::from_str::<ResponseMsg>(&text) {
                            eprintln!("Error parsing response: {}", e);
                        }
                        if let Ok(response_msg) = serde_json::from_str::<ResponseMsg>(&text) {
                            let request_id = response_msg.request_id.unwrap_or(HELLO_REQ_ID);
                            let response = response_msg.response;
                            let response_type = response_msg.ty;

                            if let Some((_, tx)) = pending_responses.remove(&request_id) {
                                match response_type.as_str() {
                                    "hello_ok" => {
                                        let _ = tx.send(ResponseType::HelloOk);
                                    }
                                    "response_error" => {
                                        if let Some(error) = response_msg.error {
                                            let _ = tx.send(ResponseType::Error {
                                                message: error.message,
                                            });
                                        }
                                    }
                                    _ => {
                                        if let Some(response) = response {
                                            let _ = tx.send(response);
                                        } else {
                                            println!("{}", text);
                                        }
                                    }
                                }
                            }
                        } else {
                            println!("Received non-response message: {}", text);
                        }
                    }
                    Ok(tungstenite::Message::Close(frame)) => {
                        println!("Connection closed: {:?}", frame);
                        break;
                    }
                    Ok(other) => match other {
                        Message::Pong(_) => {
                            if let Some((_, tx)) = pending_responses.remove(&PING_REQ_ID) {
                                let _ = tx.send(ResponseType::Pong);
                            }
                        }
                        _ => {
                            println!("Received other message: {:?}", other);
                        }
                    },
                    Err(e) => {
                        eprintln!("Error in WebSocket stream: {}", e);
                        break;
                    }
                }
            }
        });
    }
    /// This is the first handshake made to the server to authenticate the client.
    async fn send_hello(&mut self, jwt: &str) -> color_eyre::Result<()> {
        let hello_msg = HelloMsg {
            ty: "hello".to_string(),
            jwt: jwt.to_string(),
        };

        let hello_msg_text = serde_json::to_string(&hello_msg)?;
        self.writer
            .send(tungstenite::Message::Text(hello_msg_text))
            .await?;

        let (tx, rx) = oneshot::channel();
        self.pending.insert(HELLO_REQ_ID, tx);

        match rx.await? {
            ResponseType::HelloOk => Ok(()),
            _ => Err(color_eyre::eyre::eyre!("Unexpected response for hello")),
        }
    }

    /// In order to execute statements, a stream needs to be active.
    pub async fn open_stream(&mut self, stream_id: i32) -> color_eyre::Result<()> {
        let request_id = self.next_request_id().await;

        let open_stream_req = OpenStreamReq {
            ty: "request".to_string(),
            request_id,
            request: OpenStreamRequest {
                ty: "open_stream".to_string(),
                stream_id,
            },
        };

        let open_stream_text = serde_json::to_string(&open_stream_req)?;
        self.writer
            .send(tungstenite::Message::Text(open_stream_text))
            .await?;

        let (tx, rx) = oneshot::channel();
        self.pending.insert(request_id, tx);

        match rx.await? {
            ResponseType::OpenStreamResp {} => Ok(()),
            _ => Err(color_eyre::eyre::eyre!(
                "Unexpected response for open_stream"
            )),
        }
    }

    /// Measure latency in milliseconds
    pub async fn send_ping(&mut self) -> color_eyre::Result<f32> {
        self.writer.send(Message::Ping(vec![])).await?;
        let (tx, rx) = oneshot::channel();
        self.pending.insert(PING_REQ_ID, tx);
        let now = Instant::now();
        match rx.await? {
            ResponseType::Pong => Ok(now.elapsed().as_millis() as f32),
            _ => Err(color_eyre::eyre::eyre!("Unexpected response for ping")),
        }
    }

    pub async fn execute_statement(
        &mut self,
        stream_id: i32,
        sql: &str,
    ) -> color_eyre::Result<StmtResult> {
        let request_id = self.next_request_id().await;

        let execute_req = ExecuteReq {
            ty: "request".to_string(),
            request_id,
            request: ExecuteRequest {
                ty: "execute".to_string(),
                stream_id,
                stmt: Statement {
                    sql: sql.to_string(),
                    args: None,
                    named_args: None,
                    want_rows: Some(true),
                },
            },
        };

        let execute_req_text = serde_json::to_string(&execute_req)?;
        self.writer
            .send(tungstenite::Message::Text(execute_req_text))
            .await?;

        let (tx, rx) = oneshot::channel();
        self.pending.insert(request_id, tx);

        match rx.await? {
            ResponseType::ExecuteResp { result } => Ok(result),
            ResponseType::Error { message } => Err(color_eyre::eyre::eyre!("{}", message)),

            _ => Err(color_eyre::eyre::eyre!(
                "Unexpected response for execute_statement"
            )),
        }
    }

    async fn next_request_id(&self) -> i32 {
        self.request_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }
}

#[derive(Debug, Serialize)]
pub struct HelloMsg {
    #[serde(rename = "type")]
    pub ty: String,
    pub jwt: String,
}

#[derive(Serialize)]
pub struct OpenStreamReq {
    #[serde(rename = "type")]
    pub ty: String,
    pub request_id: i32,
    pub request: OpenStreamRequest,
}

#[derive(Serialize)]
pub struct OpenStreamRequest {
    #[serde(rename = "type")]
    pub ty: String,
    pub stream_id: i32,
}

#[derive(Deserialize)]
pub struct ResponseMsg {
    #[serde(rename = "type")]
    pub ty: String,
    pub request_id: Option<i32>,
    pub response: Option<ResponseType>,
    pub error: Option<ErrorType>,
}
#[derive(Deserialize)]
pub struct ErrorType {
    pub message: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum ResponseType {
    Pong,
    Error {
        message: String,
    },
    #[serde(rename = "hello_ok")]
    HelloOk,
    #[serde(rename = "open_stream")]
    OpenStreamResp,
    #[serde(rename = "execute")]
    ExecuteResp {
        result: StmtResult,
    },
    // Handle other response types as needed
}

#[derive(Deserialize)]
pub struct StmtResult {
    pub cols: Vec<Column>,
    pub rows: Vec<Vec<LibSqlValue>>,
    pub affected_row_count: i64,
    pub rows_read: i64,
    pub rows_written: i64,
    pub query_duration_ms: f64,
    // Include other fields if necessary
}

#[derive(Deserialize)]
pub struct Column {
    pub name: Option<String>,
    pub decltype: Option<String>,
}

impl Display for Column {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name.as_ref().unwrap_or(&"".to_string()))
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum LibSqlValue {
    #[serde(rename = "null")]
    Null {},
    #[serde(rename = "integer")]
    Integer { value: String },
    #[serde(rename = "float")]
    Float { value: f64 },
    #[serde(rename = "text")]
    Text { value: String },
    #[serde(rename = "blob")]
    Blob { base64: String },
}

impl Display for LibSqlValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibSqlValue::Null {} => write!(f, "null"),
            LibSqlValue::Integer { value } => write!(f, "{}", value),
            LibSqlValue::Float { value } => write!(f, "{}", value),
            LibSqlValue::Text { value } => write!(f, "{}", value),
            LibSqlValue::Blob { base64 } => write!(f, "{}", base64),
        }
    }
}

#[derive(Serialize)]
pub struct ExecuteReq {
    #[serde(rename = "type")]
    pub ty: String,
    pub request_id: i32,
    pub request: ExecuteRequest,
}

#[derive(Serialize)]
pub struct ExecuteRequest {
    #[serde(rename = "type")]
    pub ty: String,
    pub stream_id: i32,
    pub stmt: Statement,
}

#[derive(Serialize)]
pub struct Statement {
    pub sql: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub named_args: Option<Vec<NamedArg>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub want_rows: Option<bool>,
}

#[derive(Serialize)]
pub struct Value {
    #[serde(rename = "type")]
    pub ty: String,
    pub value: Option<String>,
    pub base64: Option<String>,
}

#[derive(Serialize)]
pub struct NamedArg {
    pub name: String,
    pub value: Value,
}
