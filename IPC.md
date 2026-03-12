```mermaid
sequenceDiagram
    participant HW as 硬件 (UART)
    participant INT as 中断处理 (Naked Wrapper)
    participant MB as 全局信箱 (Mailbox)
    participant USER as 用户程序 (Ring 3)
    participant KERN as 系统调用 (Ring 0)

    Note over HW, KERN: 异步中断链路
    HW->>INT: 触发 IRQ 4 (串口输入)
    INT->>INT: 保存通用寄存器 (push 15 regs)
    INT->>MB: 打包 Message 并放入 Option
    INT->>INT: 恢复通用寄存器 (pop 15 regs)
    INT->>HW: 发送 EOI & iretq (切回用户态)

    Note over USER, KERN: 系统调用交互链路
    USER->>KERN: 执行 sys_recv (RAX=11)
    KERN->>MB: 提取消息 (mailbox.take())
    KERN-->>USER: 拷贝消息至用户栈并返回 (sysretq)
    
    USER->>USER: 解析消息 (Scancode -> Char)
    
    USER->>KERN: 执行 sys_print (RAX=1)
    KERN->>HW: 输出字符至 0x3F8 端口
    KERN-->>USER: 返回 (sysretq)