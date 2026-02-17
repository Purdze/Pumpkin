use super::ChunkPos;
use crate::level::SyncChunk;
use crossbeam::channel::{Receiver, Sender};
use std::sync::Mutex;
use tokio::sync::oneshot;

pub struct ChunkListener {
    single: Mutex<Vec<(ChunkPos, oneshot::Sender<SyncChunk>)>>,
    global: Mutex<Vec<Sender<(ChunkPos, SyncChunk)>>>,
}
impl Default for ChunkListener {
    fn default() -> Self {
        Self::new()
    }
}
impl ChunkListener {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            single: Mutex::new(Vec::new()),
            global: Mutex::new(Vec::new()),
        }
    }
    pub fn add_single_chunk_listener(&self, pos: ChunkPos) -> oneshot::Receiver<SyncChunk> {
        let (tx, rx) = oneshot::channel();
        self.single.lock().unwrap().push((pos, tx));
        rx
    }
    pub fn add_global_chunk_listener(&self) -> Receiver<(ChunkPos, SyncChunk)> {
        let (tx, rx) = crossbeam::channel::unbounded();
        self.global.lock().unwrap().push(tx);
        rx
    }
    pub fn process_new_chunk(&self, pos: ChunkPos, chunk: &SyncChunk) {
        let matching_singles: Vec<_> = {
            let mut single = self.single.lock().unwrap();
            let mut i = 0;
            let mut extracted = Vec::new();
            while i < single.len() {
                if single[i].0 == pos {
                    let (_, send) = single.remove(i);
                    extracted.push(send);
                    continue;
                }
                if single[i].1.is_closed() {
                    single.remove(i);
                    continue;
                }
                i += 1;
            }
            extracted
        };
        for send in matching_singles {
            let _ = send.send(chunk.clone());
        }

        let global_senders: Vec<_> = {
            let global = self.global.lock().unwrap();
            global.clone()
        };
        let mut any_failed = false;
        for sender in &global_senders {
            if sender.send((pos, chunk.clone())).is_err() {
                any_failed = true;
            }
        }
        if any_failed {
            let mut global = self.global.lock().unwrap();
            global.retain(|s| !s.is_empty());
        }
    }
}
