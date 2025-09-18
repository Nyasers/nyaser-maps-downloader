// 标准库导入
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// 第三方库导入
use tokio::time;

// 内部模块导入
use crate::log_debug;

/// 任务队列结构体 - 管理各类任务的队列和处理状态
#[derive(Debug)]
pub struct TaskQueue<T> {
    /// 任务队列，按顺序存储待处理的任务
    pub queue: VecDeque<T>,
    /// 标记队列处理是否已启动
    pub processing_started: bool,
    /// 最大并行任务数
    pub max_concurrent_tasks: u32,
    /// 当前活跃任务的集合（存储任务ID）
    pub active_tasks: HashSet<String>,
}

impl<T> Default for TaskQueue<T> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            processing_started: false,
            max_concurrent_tasks: 1,
            active_tasks: HashSet::new(),
        }
    }
}

impl<T> TaskQueue<T> {
    /// 创建新的任务队列实例
    pub fn new(max_concurrent_tasks: u32) -> Self {
        Self {
            queue: VecDeque::new(),
            processing_started: false,
            max_concurrent_tasks,
            active_tasks: HashSet::new(),
        }
    }

    /// 添加任务到队列
    pub fn add_task(&mut self, task: T) {
        self.queue.push_back(task);
    }

    /// 检查是否可以启动新任务
    pub fn can_start_new_task(&self) -> bool {
        self.active_tasks.len() < self.max_concurrent_tasks as usize
    }

    /// 从队列中取出一个任务并标记为活跃
    pub fn take_next_task(&mut self, get_task_id: impl Fn(&T) -> String) -> Option<T> {
        if self.can_start_new_task() {
            if let Some(task) = self.queue.pop_front() {
                let task_id = get_task_id(&task);
                self.active_tasks.insert(task_id);
                Some(task)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 从活跃任务集合中移除任务
    pub fn remove_active_task(&mut self, task_id: &str) {
        self.active_tasks.remove(task_id);
    }

    /// 检查队列是否为空
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty() && self.active_tasks.is_empty()
    }

    /// 获取队列长度
    #[allow(dead_code)]
    pub fn len(&self) -> (usize, usize) {
        (self.queue.len(), self.active_tasks.len())
    }
}

/// 处理队列的通用函数
///
/// # 参数
/// - `queue`: 任务队列的共享引用
/// - `process_task_fn`: 处理单个任务的函数
/// - `get_task_id_fn`: 获取任务ID的函数
/// - `sleep_duration`: 检查间隔时间（毫秒）
/// - `should_continue_fn`: 判断是否应继续处理的函数
pub async fn process_queue<T: std::marker::Send + 'static>(
    queue: Arc<Mutex<TaskQueue<T>>>,
    process_task_fn: impl Fn(T) -> (),
    get_task_id_fn: impl Fn(&T) -> String + 'static,
    sleep_duration: u64,
    should_continue_fn: impl Fn() -> bool + 'static,
) {
    // 标记队列处理已启动
    {
        let mut q = queue.lock().unwrap();
        q.processing_started = true;
        log_debug!("队列处理已启动，最大并发任务数: {}", q.max_concurrent_tasks);
    }

    // 创建一个持续运行的循环，定期检查队列并启动新任务
    loop {
        // 检查是否应该继续运行
        if !should_continue_fn() {
            log_debug!("检测到应停止处理队列，退出循环...");
            break;
        }

        // 检查是否有任务
        let has_tasks = {
            let q = queue.lock().unwrap();
            !q.queue.is_empty() || !q.active_tasks.is_empty()
        };

        // 如果没有任务，等待一段时间后继续检查
        if !has_tasks {
            time::sleep(Duration::from_millis(sleep_duration)).await;
            continue;
        }

        // 尝试启动新任务
        let maybe_task = {
            let mut q = queue.lock().unwrap();
            q.take_next_task(&get_task_id_fn)
        };

        // 如果有任务可以启动，处理该任务
        if let Some(task) = maybe_task {
            let task_id = get_task_id_fn(&task);
            log_debug!("开始处理任务 [{}]", task_id);

            // 处理任务（非阻塞）
            process_task_fn(task);

            // 创建任务处理的异步任务
            // queue_clone和task_id_clone在async move块中被使用
            let _queue_clone = queue.clone(); // 前缀下划线表示有意未使用
            let _task_id_clone = task_id.clone();

            // 注意：这里假设任务处理是异步的，实际的任务完成处理应该在任务处理函数内部完成
            // 或者在任务处理完成后调用此移除操作
            // 以下代码仅作为示例
            // queue_clone.lock().unwrap().remove_active_task(&task_id_clone);
        }

        // 为了避免CPU占用过高，让出当前线程的执行权
        std::thread::yield_now();
        time::sleep(Duration::from_millis(50)).await;
    }
}

/// 队列管理器 - 提供队列管理的高级功能
pub struct QueueManager<T> {
    pub queue: Arc<Mutex<TaskQueue<T>>>,
}

impl<T: std::marker::Send + 'static> QueueManager<T> {
    /// 创建新的队列管理器实例
    pub fn new(max_concurrent_tasks: u32) -> Self {
        Self {
            queue: Arc::new(Mutex::new(TaskQueue::new(max_concurrent_tasks))),
        }
    }

    /// 添加任务到队列
    pub fn add_task(&self, task: T) {
        self.queue.lock().unwrap().add_task(task);
    }

    /// 启动队列处理
    pub fn start_processing(
        &self,
        process_task_fn: impl Fn(T) -> () + Send + 'static,
        get_task_id_fn: impl Fn(&T) -> String + Send + Sync + 'static,
        sleep_duration: u64,
        should_continue_fn: impl Fn() -> bool + Send + 'static,
    ) {
        let queue_clone = self.queue.clone();

        // 在新的异步任务中启动队列处理
        tauri::async_runtime::spawn(async move {
            process_queue(
                queue_clone,
                process_task_fn,
                get_task_id_fn,
                sleep_duration,
                should_continue_fn,
            )
            .await;
        });
    }
}
