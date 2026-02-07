# BUG-001: CommandFuture 首次 poll 未注册 waker 导致同步命令永久挂起

## 现象

调用 `stop_provide` 或 `remove_record` 后 Future 永远不返回，程序卡死。
异步命令（`put_record`、`get_record` 等）不受影响。

## 影响范围

所有**同步完成**的命令（即 `CommandHandler::run` 中直接调用 `handle.finish(result)` 而不需要等待后续 swarm 事件的命令）。

当前受影响的命令：
- `StopProvide`
- `RemoveRecord`

## 根因分析

`CommandFuture` 的 `poll` 实现分两阶段：

1. **首次 poll**：取出 handler，构造 `CommandTask`，通过 `try_send` 发送到 event loop
2. **后续 poll**：调用 `handle.poll(cx)` 注册 waker 并检查结果

问题在于首次 poll 的代码：

```rust
// 修复前
if let Some(handler) = this.handler.take() {
    let task = CommandTask::new(handler, this.handle.clone());
    match this.sender.try_send(Box::new(task)) {
        Ok(_) => return Poll::Pending,  // <-- 直接返回，未注册 waker
        Err(_) => { return Poll::Ready(Err(...)); }
    }
}
this.handle.poll(cx)  // 只有后续 poll 才会到达这里
```

`return Poll::Pending` 跳过了 `handle.poll(cx)`，导致 waker 未注册。

### 异步命令为什么没问题？

异步命令（如 `PutRecord`）的 `run` 方法只是向 swarm 发起查询，结果在后续 swarm 事件中通过 `on_event` 到达并调用 `handle.finish()`。由于 `try_send` 和事件处理之间存在时间差，tokio 会在下一次 poll 时进入 `handle.poll(cx)` 注册 waker，因此 `finish` 时能正常唤醒。

### 同步命令为什么卡死？

同步命令的 `run` 方法**立即**调用 `handle.finish(result)`。执行时序：

```
tokio poll CommandFuture
  -> handler.take() 取出 handler
  -> try_send(task) 发送到 event loop channel
  -> return Poll::Pending（waker 未注册）

event loop 处理 command
  -> task.run(&mut swarm)
  -> handler.run() 内部直接调用 handle.finish(result)
  -> finish() 尝试 waker.wake()，但 waker 为 None
  -> 唤醒信号丢失

tokio 不会再 poll 这个 Future（没有 waker 触发）
  -> 永久挂起
```

## 修复

首次 poll 发送命令成功后，不再 `return Poll::Pending`，而是 fall through 到 `handle.poll(cx)`：

```rust
// 修复后
if let Some(handler) = this.handler.take() {
    let task = CommandTask::new(handler, this.handle.clone());
    if this.sender.try_send(Box::new(task)).is_err() {
        return Poll::Ready(Err(...));
    }
    // Ok 时不 return，继续执行到 handle.poll(cx)
}
// 首次和后续 poll 都会注册 waker
this.handle.poll(cx)
```

这样即使同步命令在 `try_send` 后立即在另一个 task 中完成，waker 也已就绪。

## 涉及文件

- `core/src/client/future.rs`
