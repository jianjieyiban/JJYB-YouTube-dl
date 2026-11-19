use crate::scheduling::concurrency::DynamicSemaphore;
use crate::scheduling::numbering::NumberingManager;
use futures::Future;
use std::{collections::VecDeque, sync::Arc};
use tauri::{AppHandle, Runtime};
use tokio::sync::mpsc;

pub trait DispatchEntry: Clone + Send + Sync + 'static {
  fn group_id(&self) -> &String;
  fn is_running(&self) -> bool;
  fn group_key(&self) -> Option<&String>;
  fn set_numbering(&mut self, autonumber: u64, group_autonumber: Option<u64>);
}

#[derive(Clone)]
pub enum DispatchRequest<Req> {
  Pipeline(Req),
  // Recheck queued entries' own cancellation state, without targeting a group ID.
  Wake,
}

pub struct GenericDispatcher<Req> {
  sender: mpsc::UnboundedSender<DispatchRequest<Req>>,
}

impl<Req> GenericDispatcher<Req>
where
  Req: Send + Sync + Clone + 'static,
{
  pub fn start<Entry, MakeEntries, RunJob, Fut, R>(
    app_handle: AppHandle<R>,
    semaphore: Arc<DynamicSemaphore>,
    make_entries: MakeEntries,
    run_job: RunJob,
  ) -> Self
  where
    R: Runtime,
    Entry: DispatchEntry,
    MakeEntries: Fn(Req) -> Vec<Entry> + Send + Sync + 'static,
    RunJob: Fn(mpsc::UnboundedSender<DispatchRequest<Req>>, AppHandle<R>, Entry) -> Fut
      + Send
      + Sync
      + 'static,
    Fut: Future<Output = ()> + Send + 'static,
  {
    let (tx, mut rx) = mpsc::unbounded_channel::<DispatchRequest<Req>>();
    let tx_loop = tx.clone();
    let sem = semaphore;
    let app_main = app_handle;
    let make_entries = Arc::new(make_entries);
    let run_job = Arc::new(run_job);

    tauri::async_runtime::spawn(async move {
      let mut numbering = NumberingManager::new();
      let mut queues: VecDeque<(String, VecDeque<Entry>)> = VecDeque::new();
      let mut pending_requeue: Option<(String, VecDeque<Entry>)> = None;
      let mut acquired = None;
      let mut incoming = None;
      loop {
        let first = if incoming.is_some() {
          incoming.take()
        } else if queues.is_empty() && pending_requeue.is_none() {
          drop(acquired.take());
          let Some(req) = rx.recv().await else { break };
          Some(req)
        } else {
          None
        };
        // Take a snapshot so a steady stream of requests cannot starve dispatch.
        let ready = rx.len();
        for req in first
          .into_iter()
          .chain((0..ready).map_while(|_| rx.try_recv().ok()))
        {
          match req {
            DispatchRequest::Wake => {}
            DispatchRequest::Pipeline(inner) => {
              let mut entries = make_entries(inner);
              if !entries.is_empty() {
                let gid = entries[0].group_id().clone();
                if entries[0].is_running() {
                  for entry in entries.iter_mut() {
                    let group_key = entry.group_key();
                    let (autonumber, group_autonumber) = numbering.assign_for(group_key);
                    entry.set_numbering(autonumber, group_autonumber);
                  }

                  let q: VecDeque<Entry> = entries.into_iter().collect();
                  queues.push_back((gid, q));
                }
              }
            }
          }
        }

        let is_running = |(_, queue): &(String, VecDeque<Entry>)| {
          queue.front().is_some_and(DispatchEntry::is_running)
        };
        queues.retain(is_running);
        pending_requeue = pending_requeue.filter(is_running);
        if queues.is_empty() && pending_requeue.is_none() {
          continue;
        }

        let Some(permit) = acquired.take() else {
          tokio::select! {
            biased;
            permit = sem.acquire_owned() => acquired = Some(permit),
            req = rx.recv() => {
              let Some(req) = req else { break };
              incoming = Some(req);
            }
          }
          // Check arrivals and cancellation again before spending the permit.
          continue;
        };

        // Requests already waiting for a turn precede the group just served.
        queues.extend(pending_requeue.take());
        let (group_id, mut q) = queues.pop_front().unwrap();
        if !q.front().is_some_and(DispatchEntry::is_running) {
          continue;
        }

        // Grab the entry that is going to be run.
        let entry = q.pop_front().unwrap();

        tracing::trace!(
          group_id = %group_id,
          remaining_in_group = q.len(),
          "Dispatcher: scheduling entry"
        );

        let entry_group_id = entry.group_id().clone();

        // Dispatch the entry with the permit.
        {
          let tx_job = tx_loop.clone();
          let app2 = app_main.clone();
          let run_job = run_job.clone();
          tauri::async_runtime::spawn(async move {
            tracing::trace!(
              group_id = %entry_group_id,
              "Dispatcher: starting entry"
            );
            run_job(tx_job, app2, entry.clone()).await;
            tracing::trace!(
              group_id = %entry_group_id,
              "Dispatcher: finished entry, releasing permit"
            );
            drop(permit);
          });
        }

        // Requeue if more groups are left.
        if !q.is_empty() {
          pending_requeue = Some((group_id, q));
        }
      }
    });

    Self { sender: tx }
  }

  pub fn sender(&self) -> mpsc::UnboundedSender<DispatchRequest<Req>> {
    self.sender.clone()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scheduling::concurrency::DynamicSemaphore;
  use std::sync::atomic::{AtomicUsize, Ordering};
  use std::sync::Arc;
  use tauri::test::mock_app;
  use tokio::sync::{Mutex, Notify};
  use tokio::time::{sleep, timeout, Duration};

  #[derive(Clone)]
  struct TestEntry {
    group_id: String,
    index: usize,
  }

  impl DispatchEntry for TestEntry {
    fn group_id(&self) -> &String {
      &self.group_id
    }
    fn is_running(&self) -> bool {
      true
    }
    fn group_key(&self) -> Option<&String> {
      None
    }
    fn set_numbering(&mut self, _autonumber: u64, _group_autonumber: Option<u64>) {}
  }

  async fn run_concurrency_test(max_concurrency: usize, total: usize) -> usize {
    let app = mock_app();
    let group_id = "test-group".to_string();

    let sem = Arc::new(DynamicSemaphore::new(max_concurrency));
    let current = Arc::new(AtomicUsize::new(0));
    let max_seen = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(Notify::new());

    let current_job = current.clone();
    let max_job = max_seen.clone();
    let completed_job = completed.clone();
    let done_job = done.clone();

    let dispatcher = GenericDispatcher::start(
      app.handle().clone(),
      sem,
      {
        let group_id = group_id.clone();
        move |count: usize| {
          (0..count)
            .map(|index| TestEntry {
              group_id: group_id.clone(),
              index,
            })
            .collect()
        }
      },
      move |_tx, _app, _entry: TestEntry| {
        let current_job = current_job.clone();
        let max_job = max_job.clone();
        let completed_job = completed_job.clone();
        let done_job = done_job.clone();
        async move {
          let in_flight = current_job.fetch_add(1, Ordering::SeqCst) + 1;
          loop {
            let prev_max = max_job.load(Ordering::SeqCst);
            if in_flight > prev_max {
              if max_job
                .compare_exchange(prev_max, in_flight, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
              {
                break;
              }
            } else {
              break;
            }
          }

          sleep(Duration::from_millis(30)).await;
          current_job.fetch_sub(1, Ordering::SeqCst);

          let done_count = completed_job.fetch_add(1, Ordering::SeqCst) + 1;
          if done_count == total {
            done_job.notify_one();
          }
        }
      },
    );

    dispatcher
      .sender()
      .send(DispatchRequest::Pipeline(total))
      .unwrap();

    timeout(Duration::from_secs(5), done.notified())
      .await
      .expect("dispatcher did not complete within timeout");

    max_seen.load(Ordering::SeqCst)
  }

  #[tokio::test]
  async fn dispatcher_respects_single_concurrency() {
    let max_seen = run_concurrency_test(1, 3).await;
    assert_eq!(max_seen, 1);
  }

  #[tokio::test]
  async fn dispatcher_allows_two_concurrent_jobs() {
    let max_seen = run_concurrency_test(2, 4).await;
    assert_eq!(max_seen, 2);
  }

  #[tokio::test]
  async fn dispatcher_round_robins_groups_with_single_permit() {
    let app = mock_app();
    let group_a = "group-a".to_string();
    let group_b = "group-b".to_string();

    let sem = Arc::new(DynamicSemaphore::new(1));
    let initial_permit = sem.acquire_owned().await;
    let started: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(Notify::new());
    let total = 4usize;
    let completed = Arc::new(AtomicUsize::new(0));

    let started_job = started.clone();
    let done_job = done.clone();
    let completed_job = completed.clone();

    let dispatcher = GenericDispatcher::start(
      app.handle().clone(),
      sem,
      {
        move |req: (String, usize)| {
          let (group_id, count) = req;
          (0..count)
            .map(|index| TestEntry {
              group_id: group_id.clone(),
              index,
            })
            .collect::<Vec<_>>()
        }
      },
      move |_tx, _app, entry: TestEntry| {
        let started_job = started_job.clone();
        let done_job = done_job.clone();
        let completed_job = completed_job.clone();
        async move {
          {
            let mut guard = started_job.lock().await;
            guard.push(entry.group_id.clone());
          }
          sleep(Duration::from_millis(10)).await;
          let done_count = completed_job.fetch_add(1, Ordering::SeqCst) + 1;
          if done_count == total {
            done_job.notify_one();
          }
        }
      },
    );

    dispatcher
      .sender()
      .send(DispatchRequest::Pipeline((group_a.clone(), 2)))
      .unwrap();
    dispatcher
      .sender()
      .send(DispatchRequest::Pipeline((group_b.clone(), 2)))
      .unwrap();
    drop(initial_permit);

    timeout(Duration::from_secs(5), done.notified())
      .await
      .expect("dispatcher did not complete within timeout");

    let order = started.lock().await;
    assert_eq!(*order, [group_a.clone(), group_b.clone(), group_a, group_b]);
  }

  #[tokio::test]
  async fn dispatcher_does_not_starve_new_group() {
    let app = mock_app();
    let group_a = "group-a".to_string();
    let group_b = "group-b".to_string();

    let sem = Arc::new(DynamicSemaphore::new(1));
    let started: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(Notify::new());
    let completed = Arc::new(AtomicUsize::new(0));
    let total = 4usize;
    let gate = Arc::new(Notify::new());
    let gate_open = Arc::new(AtomicUsize::new(0));

    let started_job = started.clone();
    let done_job = done.clone();
    let completed_job = completed.clone();
    let gate_job = gate.clone();
    let gate_open_job = gate_open.clone();
    let group_a_run = group_a.clone();

    let dispatcher = GenericDispatcher::start(
      app.handle().clone(),
      sem,
      {
        move |req: (String, usize)| {
          let (group_id, count) = req;
          (0..count)
            .map(|index| TestEntry {
              group_id: group_id.clone(),
              index,
            })
            .collect::<Vec<_>>()
        }
      },
      move |_tx, _app, entry: TestEntry| {
        let started_job = started_job.clone();
        let done_job = done_job.clone();
        let completed_job = completed_job.clone();
        let gate_job = gate_job.clone();
        let gate_open_job = gate_open_job.clone();
        let group_a = group_a_run.clone();
        async move {
          {
            let mut guard = started_job.lock().await;
            guard.push(format!("{}:{}", entry.group_id, entry.index));
          }

          if entry.group_id == group_a
            && entry.index == 0
            && gate_open_job.fetch_add(1, Ordering::SeqCst) == 0
          {
            gate_job.notified().await;
          }

          sleep(Duration::from_millis(10)).await;
          let done_count = completed_job.fetch_add(1, Ordering::SeqCst) + 1;
          if done_count == total {
            done_job.notify_one();
          }
        }
      },
    );

    dispatcher
      .sender()
      .send(DispatchRequest::Pipeline((group_a.clone(), 3)))
      .unwrap();

    timeout(Duration::from_secs(5), async {
      loop {
        if gate_open.load(Ordering::SeqCst) > 0 {
          break;
        }
        sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .expect("first job did not start in time");

    dispatcher
      .sender()
      .send(DispatchRequest::Pipeline((group_b.clone(), 1)))
      .unwrap();
    gate.notify_one();

    timeout(Duration::from_secs(5), done.notified())
      .await
      .expect("dispatcher did not complete within timeout");

    let order = started.lock().await;
    let idx_b = order
      .iter()
      .position(|v| v.starts_with(&group_b))
      .expect("expected group B to start");
    assert_eq!(
      idx_b, 1,
      "expected new group to take the next turn: {order:?}"
    );
  }
}
