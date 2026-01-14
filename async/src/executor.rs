

// pub struct Executor<const N: usize> {
//     core_id: usize,
//     // 你的 st3 Worker
//     worker: Worker<*const TaskHeader, N>,
//     // 其他核心的 Stealer 集合，用于偷取
//     stealers: &'static [Stealer<*const TaskHeader, N>],
// }
// 
// impl<const N: usize> Executor<N> {
//     pub fn run_loop(&self) -> ! {
//         loop {
//             // 1. 尝试从本地弹出一个任务
//             if let Some(task_ptr) = self.worker.pop() {
//                 self.run_task(task_ptr);
//             }
//             // 2. 本地没任务，去偷
//             else if let Some(task_ptr) = self.try_steal() {
//                 self.run_task(task_ptr);
//             }
//             // 3. 彻底没活干，进入低功耗
//             else {
//                 core::hint::spin_loop(); // UEFI 下可以用 CpuPause
//             }
//         }
//     }
// }


