//! Work an action leaves to be done off the caller's thread, and the
//! messages it comes back with.
//!
//! The screens are written as "message in, state changed, [`Task`] out":
//! nothing in them blocks or starts a thread by itself, so their tests can
//! drop the task and look at the state. Whoever owns the state runs the
//! task with [`Task::run`], which hands every message it produces to
//! `dispatch` (normally: lock the state, apply the message, run what that
//! returns).

use std::sync::Arc;
use std::thread;

type Sink<'a, M> = &'a mut dyn FnMut(M);
/// Work that reports through a [`Sink`] as it goes.
type Reporting<M> = Box<dyn FnOnce(Sink<'_, M>) + Send>;

pub enum Task<M> {
    None,
    Batch(Vec<Task<M>>),
    /// A message to apply right away.
    Done(M),
    /// Blocking work on a thread of its own, then one message.
    Perform(Box<dyn FnOnce() -> M + Send>),
    /// Blocking work on a thread of its own that reports as it goes.
    Stream(Reporting<M>),
}

impl<M: Send + 'static> Task<M> {
    pub fn none() -> Self {
        Task::None
    }

    pub fn done(message: M) -> Self {
        Task::Done(message)
    }

    pub fn batch(tasks: impl IntoIterator<Item = Task<M>>) -> Self {
        Task::Batch(tasks.into_iter().collect())
    }

    /// Runs `work` on its own thread and turns its result into a message:
    /// `None` if it panicked, so the state never waits for an answer that
    /// can't come.
    pub fn perform<T>(
        work: impl FnOnce() -> T + Send + 'static,
        message: impl FnOnce(Option<T>) -> M + Send + 'static,
    ) -> Self {
        Task::Perform(Box::new(move || {
            message(std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).ok())
        }))
    }

    /// Runs `work` on its own thread; every message it sends is applied in
    /// the order sent.
    pub fn stream(work: impl FnOnce(Sink<'_, M>) + Send + 'static) -> Self {
        Task::Stream(Box::new(work))
    }

    pub fn map<N: Send + 'static>(self, f: impl Fn(M) -> N + Send + Sync + 'static) -> Task<N> {
        self.map_with(Arc::new(f))
    }

    fn map_with<N: Send + 'static>(self, f: Arc<dyn Fn(M) -> N + Send + Sync>) -> Task<N> {
        match self {
            Task::None => Task::None,
            Task::Batch(tasks) => Task::Batch(
                tasks
                    .into_iter()
                    .map(|task| task.map_with(f.clone()))
                    .collect(),
            ),
            Task::Done(message) => Task::Done(f(message)),
            Task::Perform(work) => Task::Perform(Box::new(move || f(work()))),
            Task::Stream(work) => {
                Task::Stream(Box::new(move |send| work(&mut |message| send(f(message)))))
            }
        }
    }

    pub fn is_none(&self) -> bool {
        match self {
            Task::None => true,
            Task::Batch(tasks) => tasks.iter().all(Task::is_none),
            _ => false,
        }
    }

    /// Starts the work. `dispatch` is called once per message: from this
    /// thread for [`Task::Done`], from the work's own thread otherwise.
    pub fn run(self, dispatch: Arc<dyn Fn(M) + Send + Sync>) {
        match self {
            Task::None => {}
            Task::Batch(tasks) => {
                for task in tasks {
                    task.run(dispatch.clone());
                }
            }
            Task::Done(message) => dispatch(message),
            Task::Perform(work) => {
                thread::spawn(move || dispatch(work()));
            }
            Task::Stream(work) => {
                thread::spawn(move || work(&mut |message| dispatch(message)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn collect<M: Send + 'static>(task: Task<M>, expected: usize) -> Vec<M> {
        let (sender, receiver) = mpsc::channel();
        let sender = std::sync::Mutex::new(sender);
        task.run(Arc::new(move |message| {
            let _ = sender.lock().unwrap().send(message);
        }));
        (0..expected)
            .map(|_| {
                receiver
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .expect("a message")
            })
            .collect()
    }

    #[test]
    fn every_kind_of_task_delivers_its_messages() {
        let task = Task::batch([
            Task::done(1),
            Task::perform(|| 2, |n| n.unwrap_or(0)),
            Task::none(),
        ]);
        let mut got = collect(task, 2);
        got.sort();
        assert_eq!(got, [1, 2]);

        let stream = Task::stream(|send| {
            for n in 0..3 {
                send(n);
            }
        });
        assert_eq!(collect(stream.map(|n| n * 10), 3), [0, 10, 20]);
    }

    #[test]
    fn work_that_panics_still_answers() {
        let task: Task<&str> = Task::perform(
            || -> u8 { panic!("boom") },
            |result| if result.is_none() { "died" } else { "lived" },
        );
        assert_eq!(collect(task, 1), ["died"]);
    }

    #[test]
    fn an_empty_batch_is_nothing_to_run() {
        assert!(Task::<u8>::batch([Task::none(), Task::none()]).is_none());
        assert!(!Task::batch([Task::done(1)]).is_none());
    }
}
