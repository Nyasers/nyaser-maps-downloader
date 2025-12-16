// 标准库导入
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

// 第三方库导入
use tokio::time;

// 内部模块导入
use crate::log_debug;

/// 任务队列结构体 - 管理各类任务的队列和处理状态
#[derive(Debug)]
pub struct TaskQueue<T> {
    /// 任务队列，按顺序存储待处理的任务ID
    pub waiting_tasks: VecDeque<String>,
    /// 标记队列处理是否已启动
    pub processing_started: bool,
    /// 最大并行任务数
    pub max_concurrent_tasks: u32,
    /// 当前活跃任务的ID集合
    pub active_tasks: VecDeque<String>,
    /// 所有任务的映射，存储完整的任务对象
    pub tasks: HashMap<String, T>,
}

impl<T> Default for TaskQueue<T> {
    fn default() -> Self {
        Self {
            waiting_tasks: VecDeque::new(),
            processing_started: false,
            max_concurrent_tasks: 1,
            active_tasks: VecDeque::new(),
            tasks: HashMap::new(),
        }
    }
}

impl<T> TaskQueue<T> {
    /// 创建新的任务队列实例
    pub fn new(max_concurrent_tasks: u32) -> Self {
        Self {
            waiting_tasks: VecDeque::new(),
            processing_started: false,
            max_concurrent_tasks,
            active_tasks: VecDeque::new(),
            tasks: HashMap::new(),
        }
    }

    /// 添加任务到队列
    pub fn add_task(&mut self, task_id: String, task: T) {
        self.tasks.insert(task_id.clone(), task);
        self.waiting_tasks.push_back(task_id);
    }

    /// 添加多个任务到队列
    pub fn add_tasks(&mut self, tasks: Vec<(String, T)>) {
        for (task_id, task) in tasks {
            self.add_task(task_id, task);
        }
    }

    /// 清空队列中的所有任务
    pub fn clear_tasks(&mut self) {
        self.waiting_tasks.clear();
        self.active_tasks.clear();
        self.tasks.clear();
    }

    /// 用新任务替换当前队列中的所有任务
    pub fn replace_tasks(&mut self, tasks: Vec<(String, T)>) {
        self.clear_tasks();
        self.add_tasks(tasks);
    }

    /// 检查是否可以启动新任务
    pub fn can_start_new_task(&self) -> bool {
        self.active_tasks.len() < self.max_concurrent_tasks as usize
    }

    /// 从队列中取出一个任务并标记为活跃
    pub fn take_next_task(&mut self) -> Option<String> {
        if self.can_start_new_task() {
            if let Some(task_id) = self.waiting_tasks.pop_front() {
                self.active_tasks.push_back(task_id.clone());
                Some(task_id)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 从活跃任务集合中移除任务
    pub fn remove_active_task(&mut self, task_id: &str) {
        if let Some(index) = self.active_tasks.iter().position(|id| id == task_id) {
            self.active_tasks.remove(index);
        }
        // 从任务映射中移除
        self.tasks.remove(task_id);
    }

    /// 通过ID查找任务
    pub fn find_task(&self, task_id: &str) -> Option<&T> {
        self.tasks.get(task_id)
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
    process_task_fn: impl Fn(String, &T) -> (),
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
            !q.waiting_tasks.is_empty() || !q.active_tasks.is_empty()
        };

        // 如果没有任务，等待一段时间后继续检查
        if !has_tasks {
            time::sleep(Duration::from_millis(sleep_duration)).await;
            continue;
        }

        // 尝试启动新任务
        let maybe_task = {
            queue.lock().unwrap().take_next_task()
        };

        // 如果有任务可以启动，处理该任务
        if let Some(task_id) = maybe_task {
            log_debug!("开始处理任务 [{}]", task_id);

            // 处理任务（非阻塞）
            process_task_fn(task_id.clone(), queue.lock().unwrap().find_task(&task_id).unwrap());
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
    pub fn add_task(&self, task_id: String, task: T) {
        self.queue.lock().unwrap().add_task(task_id, task);
    }

    /// 启动队列处理
    pub fn start_processing(
        &self,
        process_task_fn: impl Fn(String, &T) -> () + Send + 'static,
        sleep_duration: u64,
        should_continue_fn: impl Fn() -> bool + Send + 'static,
    ) {
        let queue_clone = self.queue.clone();

        // 在新的异步任务中启动队列处理
        tauri::async_runtime::spawn(async move {
            process_queue(
                queue_clone,
                process_task_fn,
                sleep_duration,
                should_continue_fn,
            )
            .await;
        });
    }
}
