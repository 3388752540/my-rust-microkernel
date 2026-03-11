#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Message {
    pub from: u64,      // 发送者 PID
    pub label: u64,     // 消息类型（自定义，如 1=请求，2=回复）
    pub payload: [u64; 2], // 16 字节的数据载荷
}