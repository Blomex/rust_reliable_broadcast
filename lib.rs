mod domain;
use uuid::Uuid;
extern crate crossbeam_channel;
pub use crate::broadcast_public::*;
pub use crate::executors_public::*;
pub use crate::stable_storage_public::*;
pub use crate::system_setup_public::*;
pub use domain::*;
type ClosureMsg = Box<dyn Fn() -> () + Send>;
pub mod broadcast_public {
    use crate::executors_public::ModuleRef;
    use crate::{PlainSenderMessage, StableStorage, StubbornBroadcastModule, SystemAcknowledgmentMessage, SystemBroadcastMessage, SystemMessageContent, SystemMessageHeader, SystemMessage};
    use std::collections::{HashSet, HashMap};
    use super::Uuid;

    pub trait PlainSender: Send + Sync {
        fn send_to(&self, uuid: &Uuid, msg: PlainSenderMessage);
    }

    pub trait ReliableBroadcast: Send {
        fn broadcast(&mut self, msg: SystemMessageContent);

        fn deliver_message(&mut self, msg: SystemBroadcastMessage);

        fn receive_acknowledgment(&mut self, msg: SystemAcknowledgmentMessage);
    }

    pub fn build_reliable_broadcast(
        sbeb: ModuleRef<StubbornBroadcastModule>,
        storage: Box<dyn StableStorage>,
        id: Uuid,
        processes_number: usize,
        delivered_callback: Box<dyn Fn(SystemMessage) + Send>,
    ) -> Box<dyn ReliableBroadcast> {
        //TODO: STORAGE GET ETC
        //retrieve 'pending'
        let mut  counter = 0;
        let mut pending = HashMap::new();
        let mut currently_analyzed = "pending".to_owned() + &counter.to_string();
        while storage.get(&currently_analyzed).is_some(){
            let message = MyReliableBroadcast::decode_system_broadcast_message(
                storage.get(&currently_analyzed).unwrap().as_slice());
            let header = message.message.header;
            pending.insert(header, message.clone());
            counter +=1;
            currently_analyzed = "pending".to_owned() + &counter.to_string();
            println!("Retrieved pending message {:#?}", message)
        }

        //retrieve 'delivered'
        let mut delivered = HashSet::new();
        let mut delivered_counter = 0;
        currently_analyzed = "delivered".to_owned() + &delivered_counter.to_string();
        while storage.get(&currently_analyzed).is_some(){
            let delivered_msg = MyReliableBroadcast::decode_header(storage.get(&currently_analyzed).unwrap().as_slice());
            delivered.insert(delivered_msg.clone());
            delivered_counter +=1;
            currently_analyzed = "delivered".to_owned() + &delivered_counter.to_string();
            println!("Retrieved delivered message: {:#?} ", delivered_msg);
        }
        //retrieve 'acks'
        let mut ack = HashMap::new();
        let mut ack_counter = 0;
        currently_analyzed = "ack".to_owned() + &ack_counter.to_string();
        while storage.get(&currently_analyzed).is_some(){
            let msg_header = MyReliableBroadcast::decode_header(storage.get(&currently_analyzed).unwrap().as_slice());
            let header_key = msg_header.message_source_id.to_string() + &msg_header.message_id.to_string();
            let acked_uuid_set = MyReliableBroadcast::decode_uuid_hashset(storage.get(&header_key).unwrap().as_slice());
            if acked_uuid_set.len() < processes_number/2 { //message was not delivered previously
                ack.insert(msg_header, acked_uuid_set);
            }
            ack_counter +=1;
            currently_analyzed = "ack".to_owned() + &ack_counter.to_string();
        }

        let result = Box::new(MyReliableBroadcast{
            sbeb,
            id,
            processes_number,
            storage,
            delivered_callback,
            delivered,
            pending,
            ack,
            counter,
            delivered_counter,
            ack_counter,
        });
        for (_k , v) in result.pending.clone(){
            result.sbeb.send(v);
        }
        result
    }

    struct MyReliableBroadcast{
        sbeb: ModuleRef<StubbornBroadcastModule>,
        id: Uuid,
        processes_number : usize,
        storage: Box<dyn StableStorage>,
        delivered_callback: Box<dyn Fn(SystemMessage) + Send>,
        delivered: HashSet<SystemMessageHeader>,
        pending : HashMap<SystemMessageHeader, SystemBroadcastMessage>,
        ack : HashMap<SystemMessageHeader, HashSet<Uuid>>,
        counter : i64,
        delivered_counter: i64,
        ack_counter: i64,
    }
    impl MyReliableBroadcast{
        fn parse_header(header: SystemMessageHeader) -> Vec<u8>{
            let msg_id = header.message_id.as_bytes().to_vec();
            let source_id = header.message_source_id.as_bytes().to_vec();
            [source_id, msg_id].concat()

        }
        fn parse_system_message_content(msg: SystemMessageContent) ->Vec<u8>{
            msg.msg
        }
        fn parse_system_message(msg: SystemMessage) -> Vec<u8>{
            let header = MyReliableBroadcast::parse_header(msg.header);
            let content = MyReliableBroadcast::parse_system_message_content(msg.data);
            [header, content].concat()
        }
        fn parse_system_broadcast_message(msg: SystemBroadcastMessage)-> Vec<u8>{
            let message = MyReliableBroadcast::parse_system_message(msg.message);
            let forwarder = msg.forwarder_id.as_bytes().to_vec();
            [forwarder, message].concat()
        }
         fn parse_hashset_uuid(set: HashSet<Uuid>) -> Vec<u8>{
             set.into_iter().fold(Vec::new(), |mut acc, v| {
                 acc.extend_from_slice(v.as_bytes());
                 acc
             })
         }
         fn decode_uuid_hashset(bytes: &[u8]) -> HashSet<Uuid>{
             bytes.chunks_exact(16).map(Uuid::from_slice).collect::<Result<_, _>>().unwrap()
         }
        fn decode_system_message_content(content: &[u8]) -> SystemMessageContent{
            SystemMessageContent{
                msg: content.to_vec()
            }
        }

        fn decode_header(header: &[u8]) -> SystemMessageHeader{
            SystemMessageHeader{
                message_source_id: Uuid::from_slice(&header[0..16]).unwrap(),
                message_id: Uuid::from_slice(&header[16..32]).unwrap(),
            }
        }
        fn decode_system_broadcast_message(message: &[u8]) -> SystemBroadcastMessage{
            SystemBroadcastMessage{
                forwarder_id: Uuid::from_slice(&message[0..16]).unwrap(),
                message: MyReliableBroadcast::decode_system_message(&message[16..]),
            }
        }
        fn decode_system_message(message: &[u8]) -> SystemMessage{
            SystemMessage{
                header: MyReliableBroadcast::decode_header(&message[0..32]),
                data: MyReliableBroadcast::decode_system_message_content(&message[32..]),
            }
        }
    }
    impl ReliableBroadcast for MyReliableBroadcast{
        fn broadcast(&mut self, msg: SystemMessageContent) {
            let header = SystemMessageHeader{
                message_source_id: self.id,
                message_id: Uuid::new_v4(),
            };
            let message = SystemMessage{
                header,
                data: msg,
            };
            let broadcast_msg = SystemBroadcastMessage{
                forwarder_id: self.id,
                message
            };
            self.pending.insert(header, broadcast_msg.clone());
            let encoded_message = MyReliableBroadcast::parse_system_broadcast_message(broadcast_msg.clone());
            let pending_messages = "pending".to_owned() + &self.counter.to_string();
            self.counter += 1;
            self.storage.put(&*pending_messages, encoded_message.as_slice()).unwrap_or(());
            self.sbeb.send(broadcast_msg)
        }

        fn deliver_message(&mut self, msg: SystemBroadcastMessage) {
            let header = msg.message.header;
            let forwarded = msg.forwarder_id;
            if !self.pending.contains_key(&header){
                self.pending.insert(header, msg.clone());
                //store new value
                let pending_messages = "pending".to_owned() + &self.counter.to_string();
                self.counter +=1;
                let encoded_message = MyReliableBroadcast::parse_system_broadcast_message(msg.clone());
                self.storage.put(&*pending_messages, encoded_message.as_slice()).unwrap_or(());
                //broadcast the message
                let new_forwarder = self.id;
                self.sbeb.send(SystemBroadcastMessage{
                    forwarder_id: new_forwarder,
                    message: msg.clone().message,
                });
            }
            if self.ack.contains_key(&header) {
                let acked_set = self.ack.get_mut(&header).unwrap();
                if !acked_set.clone().contains(&forwarded){
                    acked_set.insert(forwarded);
                    //store acked set
                    let key = header.message_source_id.to_string() + &header.message_id.to_string();
                    if self.storage.get(&key).is_none(){
                        let acked_msg = "ack".to_owned() + &self.ack_counter.to_string();
                        self.storage.put(&key, MyReliableBroadcast::parse_hashset_uuid(acked_set.clone()).as_slice()).unwrap_or(());
                        self.storage.put(&acked_msg, MyReliableBroadcast::parse_header(header).as_slice()).unwrap_or(());
                    }
                    else {
                        self.storage.put(&key, MyReliableBroadcast::parse_hashset_uuid(acked_set.clone()).as_slice()).unwrap_or(());
                    }
                    if acked_set.len() > self.processes_number/2{
                        if !self.delivered.contains(&header){
                            self.delivered.insert(header);
                            let delivered_msg_key = "delivered".to_owned() + &self.delivered_counter.to_string();
                            //call delivered callback
                            (self.delivered_callback)(msg.message);
                            //now we mark it as delivered in stable storage
                            let encoded_delivered_msg_header = MyReliableBroadcast::parse_header(header);
                            self.storage.put(&*delivered_msg_key, encoded_delivered_msg_header.as_slice()).unwrap_or(());
                        }
                    }
                }
            }
        }

        fn receive_acknowledgment(&mut self, msg: SystemAcknowledgmentMessage) {
            self.sbeb.send((self.id, msg));
        }
    }
    pub trait StubbornBroadcast: Send {
        fn broadcast(&mut self, msg: SystemBroadcastMessage);

        fn receive_acknowledgment(&mut self, proc: Uuid, msg: SystemMessageHeader);

        fn send_acknowledgment(&mut self, proc: Uuid, msg: SystemAcknowledgmentMessage);

        fn tick(&mut self);
    }

    struct MyStubbornBroadcast{
        link : Box <dyn PlainSender>,
        processes: HashSet<Uuid>,
        messages: HashMap<SystemMessageHeader, SystemBroadcastMessage>,
        pending: HashMap<SystemMessageHeader, HashSet<Uuid>>
    }
    impl StubbornBroadcast for MyStubbornBroadcast {

        fn broadcast(&mut self, msg: SystemBroadcastMessage) {
            self.messages.insert(msg.message.header, msg.clone());
            let new_set = self.processes.clone();
            self.pending.insert(msg.message.header, new_set);
            self.tick();
        }

        fn receive_acknowledgment(&mut self, proc: Uuid, msg: SystemMessageHeader) {
            let pending_set = self.pending.get(&msg).cloned();
            match pending_set{
                None => return,
                Some(mut set) => {
                    if set.contains(&proc){
                        set.remove(&proc);
                    }
                    if set.is_empty(){
                        self.pending.remove(&msg);
                    }
                    self.pending.insert(msg, set);
                }
            }
        }

        fn send_acknowledgment(&mut self, proc: Uuid, msg: SystemAcknowledgmentMessage) {
            self.link.send_to(&proc, PlainSenderMessage::Acknowledge(msg));
        }

        fn tick(&mut self) {
            for (k, v) in self.pending.clone(){
                let message = self.messages.get(&k);
                match message {
                    None => continue,
                    Some(broadcast_message) => {
                        for proc in v{
                            self.link.send_to(&proc, PlainSenderMessage::Broadcast(broadcast_message.clone()));
                        }
                    }
                }
            }
        }
    }

    pub fn build_stubborn_broadcast(
        link: Box<dyn PlainSender>,
        processes: HashSet<Uuid>,
    ) -> Box<dyn StubbornBroadcast> {
        let sbeb = MyStubbornBroadcast{
            link,
            processes,
            messages: HashMap::new(),
            pending: HashMap::new(),
        };
        return Box::new(sbeb);
    }
}

pub mod stable_storage_public {
    use std::path::PathBuf;
    use std::fs::{File, create_dir_all};
    use std::io::{Read, Write};

    pub trait StableStorage: Send {
        fn put(&mut self, key: &str, value: &[u8]) -> Result<(), String>;

        fn get(&self, key: &str) -> Option<Vec<u8>>;
    }

    #[derive(Debug, Clone)]
    struct MyStableStorage{
        root_dir: PathBuf,

    }
    const BASE64_ALPHABET: [char; 64] = [
        'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
        'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l',
        'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4',
        '5', '6', '7', '8', '9', '+', '-',
    ];
    impl MyStableStorage{
        //encoding algorithm based on https://levelup.gitconnected.com/implementing-base64-in-rust-34ef6db1e73a
        fn encode_key(content : &str) -> String{
            let characters: &[u8] = content.as_bytes();
            let mut base64_output = Vec::with_capacity((characters.len() / 3 + 1) * 4);

            let mut counter = 0;
            while counter + 3 <= characters.len() {
                let first_base64_character = MyStableStorage::extract_first_character_bits(characters[counter]);
                let second_base64_character =
                    MyStableStorage::extract_second_character_bits(characters[counter], characters[counter + 1]);
                let third_base64_character =
                    MyStableStorage::extract_third_character_bits(characters[counter + 1], characters[counter + 2]);
                let fourth_base64_character = characters[counter + 2] & 0b00111111;

                base64_output.append(&mut vec![
                    BASE64_ALPHABET[first_base64_character as usize],
                    BASE64_ALPHABET[second_base64_character as usize],
                    BASE64_ALPHABET[third_base64_character as usize],
                    BASE64_ALPHABET[fourth_base64_character as usize],
                ]);

                counter += 3;
            }

            if counter + 1 == characters.len() {
                let first_base64_character = MyStableStorage::extract_first_character_bits(characters[counter]);
                let second_base64_character = MyStableStorage::extract_second_character_bits(characters[counter], 0);

                base64_output.append(&mut vec![
                    BASE64_ALPHABET[first_base64_character as usize],
                    BASE64_ALPHABET[second_base64_character as usize],
                    '=',
                    '=',
                ]);
            } else if counter + 2 == characters.len() {
                let first_base64_character = MyStableStorage::extract_first_character_bits(characters[counter]);
                let second_base64_character =
                    MyStableStorage::extract_second_character_bits(characters[counter], characters[counter + 1]);
                let third_base64_character = MyStableStorage::extract_third_character_bits(characters[counter + 1], 0);

                base64_output.append(&mut vec![
                    BASE64_ALPHABET[first_base64_character as usize],
                    BASE64_ALPHABET[second_base64_character as usize],
                    BASE64_ALPHABET[third_base64_character as usize],
                    '=',
                ]);
            }

            base64_output.into_iter().collect::<String>()
        }
        fn extract_first_character_bits(first_byte: u8) -> u8 {
            (first_byte & 0b1111100) >> 2
        }

        fn extract_second_character_bits(first_byte: u8, second_byte: u8) -> u8 {
            (first_byte & 0b00000011) << 4 | ((second_byte & 0b11110000) >> 4)
        }

        fn extract_third_character_bits(second_byte: u8, third_byte: u8) -> u8 {
            (second_byte & 0b00001111) << 2 | ((third_byte & 0b11000000) >> 6)
        }
    }
    const MAX_KEY_SIZE: usize = 255;
    impl StableStorage for MyStableStorage{
        fn put(&mut self, key: &str, value: &[u8]) -> Result<(), String> {
            print!("Key: {}\n", key);
            if key.len() > MAX_KEY_SIZE{
                return Err("Key is too big".to_owned());
            }
            if value.len() > 655536 {
                return Err("value size is too big".to_owned());
            }
            let key = key.to_owned() + "!";
            let mut path = PathBuf::new();
            path.push(self.root_dir.clone());
            let encoded_key = MyStableStorage::encode_key(&key);
            if encoded_key.len() > MAX_KEY_SIZE {
                path.push( &encoded_key[..MAX_KEY_SIZE/2]);
                create_dir_all(path.clone()).map_err(|e| e.to_string())?;
                path.push(&encoded_key[MAX_KEY_SIZE/2..])
            }
            else {
                path.push( &*encoded_key);
            }
            print!("Path when PUSHING: {:#?}\n", path);
            match File::create(path){
                Ok(mut file) => {
                    match file.write_all(value) {
                        Ok(_) => return Ok(()),
                        Err(_) => return Err("couldn't write to file".to_owned())
                    }
                }
                Err(reason) => return Err(reason.to_string()),
            }
        }
        fn get(&self, key: &str) -> Option<Vec<u8>> {
            if key.len() > MAX_KEY_SIZE {
                return None;
            }
            let key = key.to_owned() + "!";
            let mut path = self.root_dir.clone();
            print!("root dir {:#?}\n", self.root_dir);
            let encoded_key = MyStableStorage::encode_key(&key);
            if encoded_key.len() > MAX_KEY_SIZE {
                path.push(&encoded_key[..MAX_KEY_SIZE/2]);
                path.push(&encoded_key[MAX_KEY_SIZE/2..])
            }
            else {
                path.push(&*encoded_key);
            }
            print!("path when GETTING {:#?}\n", path);
            let mut file = match File::open(path){
                Err(_) => return None,
                Ok(file) => file,
            };
            let mut buf: Vec<u8> = vec![];
            return match file.read_to_end(buf.as_mut()) {
                Err(_) => None,
                Ok(_) => Some(buf.to_vec()),
            }
        }
    }

    pub fn build_stable_storage(root_storage_dir: PathBuf) -> Box<dyn StableStorage> {
        Box::new(MyStableStorage{
            root_dir: root_storage_dir,
        })
    }
}

pub mod executors_public {
    use std::{fmt, thread};
    use std::time::{Duration, Instant};
    use crossbeam_channel::{unbounded, Receiver, Sender};
    use crate::{ClosureMsg};
    use std::sync::{Arc, Mutex, Condvar, Weak};
    use std::ops::{DerefMut};
    use std::thread::{JoinHandle};
    use uuid::Uuid;
    use std::collections::{BinaryHeap, HashMap};
    use std::cmp::{Ordering};
    use std::panic::catch_unwind;

    pub trait Message: fmt::Debug + Clone + Send + 'static {}
    impl<T: fmt::Debug + Clone + Send + 'static> Message for T {}


    pub trait Handler<M: Message>
    where
        M: Message,
    {
        fn handle(&mut self, msg: M);
    }

    #[derive(Debug, Clone)]
    pub struct Tick {}

    pub struct System {
        tick: Option<JoinHandle<()>>,
        executor: Option<JoinHandle<()>>,
        tick_handler: Arc<(Mutex<BinaryHeap<Ticker>>, Condvar)>,
        pub(crate) meta_tx: crossbeam_channel::Sender<WorkerMsg>,
        starting_time: Instant,
        done: Arc<Mutex<bool>>,
    }

    struct Ticker{
      //  uuid: Uuid,
        next_time: Duration,
        tick_duration: Duration,
        tick_ref: Weak<Mutex<dyn std::marker::Send + Handler<Tick>>>,
    }
    impl Ord for Ticker{
        fn cmp(&self, other: &Self) -> Ordering {
            //reverse order
            return other.next_time.cmp(&self.next_time);
        }
    }
    impl Eq for Ticker{}
    impl PartialEq for Ticker{
        fn eq(&self, other: &Self) -> bool {
            self.next_time == other.next_time &&
                self.tick_duration == other.tick_duration &&
                Weak::ptr_eq(&self.tick_ref, &other.tick_ref)
        }
    }
    impl PartialOrd for Ticker{
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }
    pub(crate) enum WorkerMsg {
        NewModule(Uuid, Arc<Mutex<dyn Send + 'static>>),
        /// When all references to a module are dropped, the module ought to be
        /// removed too. Otherwise, there is a possibility of a memory leak.
        RemoveModule(Uuid),

        ///message sent to executor thread to inform him that he is supposed to finish
        ExecutionDone,
        /// This message could be used to notify the system, that there is a message
        /// to be deliver to the specified module.
        ExecuteModule(ClosureMsg),
    }
    impl System {
        pub fn request_tick<T: Send + Handler<Tick> + std::marker::Send>(&mut self, requester: &ModuleRef<T>, delay: Duration) {
            let mut tick_mutex = (*self.tick_handler).0.lock().unwrap();
            let dummy_ticker = tick_mutex.deref_mut();
            let current_time = Instant::now();
            let ticker = Ticker{
                tick_ref: requester.module.clone(),
                tick_duration: delay,
                next_time: current_time.duration_since(self.starting_time) + delay,
            };
            dummy_ticker.push(ticker);
            print!("Tick request registered!\n");
            (*self.tick_handler).1.notify_all();
        }

        pub fn register_module<T: Send + 'static>(&mut self, module: T) -> ModuleRef<T> {
            let uuid = Uuid::new_v4();
            let module_ptr = Arc::new(Mutex::new(module));
            self.meta_tx.send(WorkerMsg::NewModule(uuid, module_ptr.clone())).unwrap_or(());
            ModuleRef{
                    ref_counter: Arc::new(Mutex::new(1)),
                    uuid,
                    module: Arc::downgrade(&module_ptr),
                    meta_queue: self.meta_tx.clone(),
                }
        }

        pub fn new() -> Self {
            let (meta_tx, meta_rx):  (Sender<WorkerMsg>, Receiver<WorkerMsg>) = unbounded();
            let now_timer = Instant::now();
            let done = Arc::new(Mutex::new(false));
            let tick_handler : Arc<(Mutex<BinaryHeap<Ticker>>, Condvar)> =
                Arc::new((Mutex::new(BinaryHeap::new()), Condvar::new()));

            // let mut system =  System{
            //     tick: None,
            //     executor: None,
            //     tick_handler: Arc::new((Mutex::new(BinaryHeap::new()), Condvar::new())),
            //     meta_tx,
            //     starting_time: now_timer,
            //     done: Arc::new(Mutex::new(false)),
            // };
            let cloned_tick_handler = tick_handler.clone();
            //let cloned_tick_handler2 = system.tick_handler.clone();
            let cloned_done = done.clone();
            /*system.executor = Option::from(spawn(move || {
                let cloned_done = cloned_done.clone();
                while let Ok(msg) = meta_rx.recv(){
                    match msg{
                        WorkerMsg::NewModule(uuid) => {
                            println!("new module {}", uuid);
                            // cloned_modules.insert(uuid);
                        },
                        WorkerMsg::RemoveModule(_uuid) =>{
                            // println!("removing module {}", uuid);
                            // cloned_modules.remove(&uuid);
                            // //find 'ticker'
                            // let (mutex, cond) = &*cloned_tick_handler2;
                            // let mut guard = mutex.lock().unwrap();
                            // let heap = guard.deref_mut();
                            // let mut new_heap = BinaryHeap::new();
                            // while !heap.is_empty(){
                            //     let ticker = heap.pop().unwrap();
                            //     if ticker.uuid != uuid{
                            //         new_heap.push(ticker);
                            //     }
                            // }
                            // for item in new_heap{
                            //     heap.push(item);
                            // }
                        },
                        WorkerMsg::ExecuteModule(closure) => {
                            closure();
                        }
                    }
                    let mut done_lock = cloned_done.lock().unwrap();
                    let guard = done_lock.deref_mut();
                    if *guard == true {
                        println!("Executor is ending");
                        return;
                    }

                }
                print!("executor thread exiting..\n");
            }));*/
            let now = now_timer;
            /*system.tick = Option::from(spawn(move || {
                let done = cloned_done.clone();
                let mut time = Duration::from_secs(0);
                let predicate = Box::new(|var: &mut BinaryHeap<Ticker>, time: &mut Duration|{
                    if !var.is_empty() {
                        let current_time = Instant::now();
                        let shortest_time = var.peek().unwrap();
                        let elapsed = current_time.duration_since(now);
                        if elapsed.gt(&shortest_time.next_time){
                            println!("dur: {:#?}", current_time.duration_since(now));
                            return true;
                        }
                        else {
                            *time = shortest_time.next_time - elapsed;
                        }
                        return false;
                    }
                    return false;
                });
                let (mutex, cond) = &*cloned_tick_handler;
                loop {
                    let mut guard = mutex.lock().unwrap();
                    while !predicate(guard.deref_mut(), &mut time) {
                        guard = cond.wait_timeout(guard, time).unwrap().0;
                      //  println!("keep waiting")
                    }

                    // let start_time = SystemTime::now();
                    let ticker = guard.deref_mut();
                    let mut first_module = ticker.pop().unwrap();
                    //Trying to pick up module mutex
                    let mut module_mutex = first_module.tick_ref.lock().unwrap();
                    let module_handler = module_mutex.deref_mut();
                    println!("Tick requested! {:#?} {:#?}", first_module.next_time, first_module.tick_duration);
                    module_handler.handle(Tick {});
                    drop(module_mutex);
                    first_module.next_time += first_module.tick_duration;
                    ticker.push(first_module);
                        drop(guard);
                    let mut done_lock = done.lock().unwrap();
                    let guard = done_lock.deref_mut();
                    if *guard == true {
                        println!("Tick is ending");
                        return;
                    }
                }

            }));*/

            System{
                tick: Option::from(thread::Builder::new().name("ticker".to_string()).spawn(move || {
                    let done = cloned_done.clone();
                    let mut time = Duration::from_secs(0);
                    let predicate = Box::new(|var: &mut BinaryHeap<Ticker>, time: &mut Duration|{
                        if !var.is_empty() {
                            let current_time = Instant::now();
                            let shortest_time = var.peek().unwrap();
                            let elapsed = current_time.duration_since(now);
                            if elapsed.gt(&shortest_time.next_time){
                                println!("dur: {:#?}", current_time.duration_since(now));
                                return true;
                            }
                            else {
                                *time = shortest_time.next_time - elapsed;
                            }
                            return false;
                        }
                        return false;
                    });
                    let (mutex, cond) = &*cloned_tick_handler;
                    loop {

                        let mut guard = mutex.lock().unwrap();
                        while !predicate(guard.deref_mut(), &mut time) {
                         //   println!("Checking tick");
                            guard = cond.wait_timeout(guard, time).unwrap().0;
                            let mut done_lock = done.lock().unwrap();
                            let guard = done_lock.deref_mut();
                            println!("guard is {:#?}", guard);
                            if *guard == true {
                                println!("Tick is ending");
                                return;
                            }
                            //  println!("keep waiting")
                        }

                        // let start_time = SystemTime::now();
                        let ticker = guard.deref_mut();
                        let mut first_module = ticker.pop().unwrap();
                        //Trying to pick up module mutex
                        println!("picking mutex");
                        println!("Tick requested! {:#?} {:#?}", first_module.next_time, first_module.tick_duration);
                        let result = catch_unwind( ||{
                            let arc = first_module.tick_ref.upgrade().unwrap();
                            let mut module_mutex = arc.lock().unwrap();
                            let module_handler = module_mutex.deref_mut();
                            module_handler.handle(Tick {});
                            drop(module_mutex);
                        });
                        println!("request done");
                        //drop(module_mutex);
                        first_module.next_time += first_module.tick_duration;
                        if result.is_ok() {
                            ticker.push(first_module);
                        }
                        drop(guard);
                    }

                }).unwrap()),
                executor: Option::from(thread::Builder::new().name("executor".to_string()).spawn(move || {
                    let mut modules = HashMap::new();
                    while let Ok(msg) = meta_rx.recv(){
                        match msg{
                            WorkerMsg::NewModule(uuid, module) => {
                                println!("new module {}", uuid);
                                modules.insert(uuid, module);
                                // cloned_modules.insert(uuid);
                            },
                            WorkerMsg::RemoveModule(_uuid) =>{
                                // println!("removing module {}", uuid);
                                // cloned_modules.remove(&uuid);
                                // //find 'ticker'
                                // let (mutex, cond) = &*cloned_tick_handler2;
                                // let mut guard = mutex.lock().unwrap();
                                // let heap = guard.deref_mut();
                                // let mut new_heap = BinaryHeap::new();
                                // while !heap.is_empty(){
                                //     let ticker = heap.pop().unwrap();
                                //     if ticker.uuid != uuid{
                                //         new_heap.push(ticker);
                                //     }
                                // }
                                // for item in new_heap{
                                //     heap.push(item);
                                // }
                            },
                            WorkerMsg::ExecuteModule(closure) => {
                                closure();
                            },
                            _ => return,
                        }
                    }
                    print!("executor thread exiting..\n");
                }).unwrap()),
                tick_handler: tick_handler,
                meta_tx,
                starting_time: now_timer,
                done: done
            }
        }
    }
    impl Drop for System{
        fn drop(&mut self) {
            println!("Trying to get done mutex");
            *self.done.lock().unwrap().deref_mut() = true;
            println!("Telling all threads to end");
            self.tick_handler.1.notify_all();
            self.meta_tx.send(WorkerMsg::ExecutionDone).unwrap_or(());
            self.tick.take().unwrap().join().unwrap_or(());
            self.executor.take().unwrap().join().unwrap_or(());
        }
    }

    pub struct ModuleRef<T: Send + 'static> {
        uuid: Uuid,
        module : Weak<Mutex<T>>,
        meta_queue: Sender<WorkerMsg>,
        ref_counter: Arc<Mutex<i32>>
    }

    impl<T: Send> ModuleRef<T> {
        pub fn send<M: Message>(&self, msg: M)
        where
            T: Send + Handler<M>,
        {
            let mod_ref = self.module.clone();
            self.meta_queue
                .send(WorkerMsg::ExecuteModule(
                    Box::new(move || {
                        match mod_ref.upgrade() {
                            Some(lock) => {
                                let mut  guard = lock.lock().unwrap();
                                guard.deref_mut().handle(msg.clone());
                            }
                            None => return,
                        }
                    }))).ok();
        }
    }
    impl<T: Send> Drop for ModuleRef<T>{
        fn drop(&mut self) {
            let mut lock = self.ref_counter.lock().unwrap();
            let value = lock.deref_mut();
            *value -=1;
            if *value == 0 {
                self.meta_queue.send(WorkerMsg::RemoveModule(self.uuid)).unwrap_or(());
            }
        }
    }
    impl<T: Send> fmt::Debug for ModuleRef<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
            f.write_str("<ModuleRef>")
        }
    }
    impl<T: Send> Clone for ModuleRef<T> {
        fn clone(&self) -> Self {
            //increase ref count
            let mut lock = self.ref_counter.lock().unwrap();
            let value = lock.deref_mut();
            *value +=1;
            drop(lock);
            ModuleRef{
                ref_counter: self.ref_counter.clone(),
                uuid: self.uuid,
                module: self.module.clone(),
                meta_queue: self.meta_queue.clone(),
            }
        }
    }
}

pub mod system_setup_public {
    use crate::{Configuration, ModuleRef, ReliableBroadcast, StubbornBroadcast, System, build_stubborn_broadcast, build_reliable_broadcast};

    pub fn setup_system(
        system: &mut System,
        config: Configuration,
    ) -> ModuleRef<ReliableBroadcastModule> {
        let stubborn_broadcast: StubbornBroadcastModule =
            StubbornBroadcastModule{
                stubborn_broadcast: build_stubborn_broadcast(
                    config.sender, config.processes.clone())};

        let sbeb_ref = system.register_module(stubborn_broadcast);

        let dyn_rel_broadcast : Box<dyn ReliableBroadcast> =
            build_reliable_broadcast(sbeb_ref.clone(),
                                     config.stable_storage,
                                     config.self_process_identifier,
                                     config.processes.len(),
                                     config.delivered_callback);

        let rbm = ReliableBroadcastModule{reliable_broadcast: dyn_rel_broadcast};
        let rbm_ref = system.register_module(rbm);
        system.request_tick(&sbeb_ref, config.retransmission_delay);
        return rbm_ref;

    }

    pub struct ReliableBroadcastModule {
        pub(crate) reliable_broadcast: Box<dyn ReliableBroadcast>,
    }

    pub struct StubbornBroadcastModule {
        pub(crate) stubborn_broadcast: Box<dyn StubbornBroadcast>,
    }
}
