```mermaid
graph LR
    subgraph Virtual_Address_Space [虚拟地址空间]
        direction TB
        High_Addr["0xFFFF... (Higher Half)"]
        Kernel_Code["内核代码 / 段 (Kernel Text)"]
        Kernel_Heap["内核堆 (alloc / Vec / Box)"]
        
        Gap["... 未映射区域 (Guard Hole) ..."]
        
        User_Stack["用户态栈 (0x3000_8000)"]
        User_Code["用户态程序 (0x2000_0000)"]
        Low_Addr["0x0000..."]
    end

    subgraph Physical_RAM [物理内存]
        direction TB
        Frame_N["物理页帧 N"]
        Frame_Stack["栈物理内存"]
        Frame_ELF["ELF 代码物理内存"]
        Frame_0["物理页帧 0"]
    end

    %% 映射关系
    Kernel_Code -.->|Offset Mapping| Frame_N
    User_Code == "USER 位开启" ==> Frame_ELF
    User_Stack == "USER 位开启" ==> Frame_Stack
    
    style User_Code fill:#f96,stroke:#333,stroke-width:2px
    style User_Stack fill:#f96,stroke:#333,stroke-width:2px
    style Kernel_Code fill:#6cf,stroke:#333,stroke-width:2px