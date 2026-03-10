```mermaid
graph TD
    %% --- 用户态 (Ring 3) ---
    subgraph UserZone [用户空间 - 策略层]
        direction TB
        UI[交互式 Shell & 用户应用]
        subgraph Srv [系统服务进程]
            FS[VFS 文件系统] --- NET[网络协议栈] --- DISK[驱动服务]
        end
    end

    %% --- 权限网关 ---
    subgraph Gateway [特权级切换网关]
        direction LR
        Syscall[Syscall 高速入口] <--> IPC[异步消息总线]
    end

    %% --- 微内核核心 (Ring 0) ---
    subgraph KernelCore [微内核核心 - 机制层]
        direction TB
        
        subgraph MemMgmt [1. 内存安全管理]
            Frame[物理页分配] --> Paging[4级页表隔离] --> Heap[动态堆分配]
        end

        subgraph SMPMgmt [2. 多核对称处理]
            BSP[主核引导逻辑] --> IPI[核间中断 IPI] --> PerCPU[CPU 本地存储]
        end

        subgraph TaskMgmt [3. 异步任务调度]
            Exec[Waker 执行器] --> Ctx[寄存器现场切换]
        end
    end

    %% --- 硬件抽象层 ---
    subgraph HALZone [硬件抽象层 HAL]
        ACPI[ACPI 拓扑探测] --- APIC[高级中断控制] --- UART[串口调试驱动]
    end

    %% --- 物理层 ---
    HW((x86_64 多核物理硬件))

    %% --- 总体流向 ---
    HW ===> HALZone
    HALZone ===> KernelCore
    KernelCore <==> Gateway
    Gateway <==> UserZone

    %% 样式应用
    class HW hardware;
    class KernelCore,MemMgmt,SMPMgmt,TaskMgmt kernel;
    class UserZone,UI,Srv,FS,NET,DISK user;
    class Gateway,Syscall,IPC bridge;