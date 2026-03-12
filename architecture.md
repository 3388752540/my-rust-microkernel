```mermaid
graph TB
    subgraph User_Space [用户空间 - Ring 3]
        INIT[Init Process / Echo Server]
        LIB[User Syscall Library]
        INIT --> LIB
    end

    subgraph Kernel_Space [内核空间 - Ring 0]
        subgraph Core_Services [核心服务层]
            SYSCALL[Syscall Handler]
            IPC[Mailbox / IPC Dispatcher]
            LOADER[ELF Loader]
        end

        subgraph Execution_Engine [异步调度引擎]
            EXEC[Async Executor]
            TASK[Task / Waker System]
            EXEC <--> TASK
        end

        subgraph Memory_Management [内存管理系统]
            PAGE[Paging / Mapper]
            FRAME[Frame Allocator]
            HEAP[Kernel Heap / alloc]
        end

        subgraph Hardware_Abstraction [硬件抽象层]
            GDT[GDT / TSS / RSP0]
            IDT[IDT / Naked Wrappers]
            DRV[Serial / Timer Drivers]
        end
    end

    subgraph Hardware [硬件层]
        CPU[x86_64 CPU]
        RAM[Physical RAM]
        UART[UART / COM1]
    end

    %% 交互关系
    LIB -- "syscall" --> SYSCALL
    SYSCALL -- "sysretq" --> User_Space
    DRV -- "IRQ" --> IDT
    IDT -- "Event Waking" --> TASK
    IDT -- "Message Push" --> IPC
    IPC -- "sys_recv" --> INIT
    PAGE -- "Mapping" --> RAM
    CPU -- "Privilege Check" --> GDT