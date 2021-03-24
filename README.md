# rust_reliable_broadcast

## Background
Reliable broadcast is mostly a technical algorithm to provide higher-level meaningful abstractions. Because of this, the description will be rather dry. Nevertheless, it is a reasonably simple and important distributed algorithm. Your task is to implement the executor system and then use it to implement a variant of the reliable broadcast.

Your solution must implement a public interface from the attached template to provide a fault tolerant reliable broadcast facility. The solution should take the form of a library. You are allowed to change layout of files in the template, but you are not allowed to modify the public interface of the library.

## Problem description
### Overview
There is a fixed number of equivalent (instances of the same software) processes, knowing of each other. At any time, the processes may want to broadcast a message—requests to do so will arrive externally to your software. The processes can crash, and are expected to be able to recover from these crashes—we assume the fail-recovery model. We assume also that majority of the processes must be up and running for the system to function. In other words, we expect system to make progress in message delivery whenever more than half of the processes are up.

## Crashes
Individual processes can crash and recover, which means stable storage is necessary. In a configuration (see template/lib.rs) you are provided with a stable storage abstraction represented by a trait StableStorage. You can assume that every process has a unique stable storage and that it is indeed reliable.

The stable storage is provided conceptually as a key-value store, with a limitation that no key can be longer than 255 bytes, and stored values must have less than 65536 bytes. Implementing such stable storage backed by a directory on a filesystem is a part of your task.

## Solution interface
Now we introduce the interface of your solution. It is provided in template/lib.rs and you cannot modify it, or your solution will not build correctly against tests. By the interface, we mean here only publicly exported symbols from the library crate.

Your task is to provide a rust library crate based on the provided template. The main method is function setup_system from template/lib.rs, which is responsible for setting up your reliable broadcast solution and creating, among others modules you will need, the one implementing ReliableBroadcast trait (see template/lib.rs). All communication from the outside world will be done via a reference to ReliableBroadcastModule (template/lib.rs) which must be returned from the setup_system method. This module must implement the main algorithm (its description is provided below). Your modules must be created in System (the struct defined in template/lib.rs) which is provided as an argument to setup_system. System is described in more details closer to the end of the document. Note that every reliable broadcast process will have its own System object, and that such broadcast processes can run on different physical machines.

In the Configuration type (template/domain.rs) provided to setup_system you can find all relevant information required to launch a single process of your solution. It contains:

UUID of the process—every process has such identifier

UUIDs of every other process in the system

StableStorage (template/lib.rs) for your system to use

PlainSender (template/lib.rs) which provides a way to send a message to another process (specified by the UUID). Use it as a faulty link! It ignores unknown UUIDs.

time interval your solution should wait for an acknowledgment from other process before attempting retransmission via PlainSender

callback to call when reliable broadcast decides it is safe to deliver a message to an upper software layer

As for technical build details, you can use only crates listed in Cargo.toml in template as the main dependencies. You can use anything you want for the [dev-dependencies] section, as it will be ignored by tests. You can specify any number of binaries in your Cargo.toml, as those also will be ignored by tests. You must not use [build-dependencies].

## Logged Majority Ack uniform reliable broadcast
You are provided here with the dry description of the algorithm from the lecture. Plus denotes set sum, and empty means an empty set, N is the total number of process:
```
Implements:
    LoggedUniformReliableBroadcast, instance lurb.

Uses:
    StubEbornBestEffortBroadcast, instance sbeb. Described below.

upon event < lurb, Init > do
    delivered := empty;
    pending := empty;
    forall m do ack[m] := empty;

upon event < lurb, Recovery > do
    retrieve(pending, delivered);
    forall (s, m) `in` pending do
        trigger < sbeb, Broadcast | [DATA, s, m];

upon event < lurb, Broadcast | m > do
    pending := pending + {(self, m)};
    store(pending);
    trigger < sbeb, Broadcast | [DATA, self, m] >;

upon event < sbeb, Deliver | p, [DATA, s, m] > do
    if (s, m) `not in` pending then
        pending := pending + {(s, m)}
        store(pending);
        trigger < sbeb, Broadcast | [DATA, s, m] >;
    if p `not in` ack[m] then
        ack[m] := ack[m] + {p};
        if #(ack[m]) > N/2 `and` (s, m) `not in` delivered then
            delivered := delivered + {(s, m)};
            store(delivered);
            trigger < lurb, Deliver | delivered >;


Implements:
    StubbornBestEffortBroadcast, instance sbeb.

Uses:
    StubbornPointToPointLinks, instance sl

upon event < sbeb Broadcast | m > do
    forall q `in` all_processes
        trigger < sl, Send | q, m >;

upon event < sl, Deliver | p, m > do
    trigger < sbeb, Deliver | p, m >;
```
StubbornPointToPointLinks is a plain link that takes care of retransmissions of a message until the recipient acknowledges receiving it. In your solution use PlainSender (template/lib.rs) which is a faulty link (it can lose and duplicate messages, but it cannot corrupt them). PlainSender can send two types of messages: Acknowledge that a message was received, and Broadcast messages to other processes.

Details of message handlers were covered in the lectures, your solution must implement them in the way specified above. For the reliable broadcast, we do not distinguish between Init and Recovery—your system must always "boot" from the stable storage.

Then you must provide two functions—build_reliable_broadcast and build_stubborn_broadcast (both in template/lib.rs)—which implement these algorithms.

Delivering to upper layer
We require that when a process is ready to deliver a broadcast message (SystemMessage from template/lib.rs), you must call delivered_callback (from Configuration) when a message is to be delivered by the reliable broadcast to the upper layers of software.

Your solution should provide at-least-once semantics. That is it must invoke the callback before marking it as delivered. This might result in multiple callback invocations if there is a crash just before the message is marked as delivered, but there is no way around this issue. (In general, you either accept multiple deliveries, or you mark messages as delivered before calling the callback accepting that some messages might not be delivered at all).

Internal interfaces
Now we will describe technical requirements of the solution. We will start with explanation of the architecture. Each System is intended to host modules which implement reliable broadcast algorithm. When run, it must be connected to the environment to allow for delivery of messages by the implementation of PlainLink - for instance integration tests will wrap every System into Linux process, with communication performed over UDP in another execution thread. There is an example in public-tests/executors_main.rs which shows how the executor system can be used.

Executor system - System struct
Internally, your solution must be based on an executor engine. Executor engines were discussed in length at laboratory classes. Its interface can be found in template/lib.rs and template/domain.rs. The main entity is the System struct. Creating a System object must also activate it (i.e., it is ready to deliver messages to modules), as there is no separate start method. System allows also for registration of new modules returning ModuleRef to them, and for requesting periodic Tick message with the fixed interval. ModuleRef (via implementations of a Handler trait) allows for sending messages of multiple types to modules of arbitrary type. The Message trait defines what is expected from messages in the executor system. It must be also possible to use ModuleRef as Message.

Moreover, we require the following:

One System uses no more than two threads

After dropping the System, every thread it created has finished gracefully

Dropping the last ModuleRef must remove the module from the system (otherwise there would be a memory leak)

Ticks must be delivered at specified time intervals. There shall be no drift, and when averaged over a large time interval the frequency of ticks shall be the inverse of their specified interval.

After sending a message by using a reference to the module, the message will be delivered to the module after reasonably short delay

Your executor system does not panic under any circumstances

Note there is no API to cancel or request the timer—it will be tested by StubbornBroadcast implementation.

Architecture of the proposed system
First of all, we suggest using one thread to implement the tick facility, and a second one to implement the executor engine.

We split the issue into two problems:

implementing a simple executor system, with modules accepting only one message type
implementing multi-type executor system on top of the previous one
However, it is up to you how you implement the requirements listed in the previous sections.

Simple executor system
The simple executor system allows for creation of modules of an arbitrary type that is unknown to the engine implementation beforehand. Such modules accept only one type of messages. The messages are processed by a single executor.

A deviation from the standard event-driven shared-nothing architecture is a usage of multiple buffers: to allow typesafe code, we will use multiple buffers. In fact, every module has its own message buffer, and the executor is notified when there is a new message ready to be processed by a particular module.

This way the modules can be owned by one thread which awaits on a channel for meta operations to execute:
```rust
enum WorkerMsg {
    NewModule,
    /// When all references to a module are dropped, the module ought to be
    /// removed too. Otherwise, there is a possibility of a memory leak.
    RemoveModule,
    /// This message could be used to notify the system, that there is a message
    /// to be deliver to the specified module.
    ExecuteModule,
}
```
Multi-type executor system
Let us now consider the only message type that can be received by a module from the simple executor system (the on described above). If one chooses a closure as this type, then it is possible to move messages of any type into such closure:
```rust
fn sample<T, M>(msg: M) {
    let _closure_message = move |module: &mut T| {
        module.handle(msg);
    };
}
```
The appropriate handle method is resolved by using type bounds—that what the Handler trait is necessary for.

This concludes our brief description of the required executor system.

Stable storage trait
Your solution must also provide an implementation of StableStorage based on a filesystem directory. This implementation is to be provided by the build_stable_storage function (see template/lib.rs).

The access to the underlying directory is exclusive: one process uses one directory, and only one instance of StableStorage has access to it. Your stable storage must:

store and retrieve values – length of the key is limited to 255 bytes and values must not be longer than 65536, it returns Err on invalid inputs (the error message can be anything you want)

recover from crash – if a key was inserted before crash, after the restart your storage can return a value for the key

return no value for a key that was never inserted

store values atomically – either the whole value is stored under the key, or the value is not inserted/modified (for instance, during a crash of the operating system process)

store message content only for pending messages

You can assume that there is enough disc space, and that correct file operations do not fail. However, if your solution makes an excessive usage of the stable storage, or it produces large volumes of data to be stored, it will fail integration tests. We do not define excessive usage here, but it will be obvious if one occurs. Do NOT access any other directory then the provided one!

Reliable broadcast trait
Methods broadcast and deliver_message were discussed in the section about the main algorithm.

Our addition is receive_acknowledgment which is invoked when an acknowledgment from another process has arrived—it is pushed to ReliableBroadcast for convenience, so that whole inward communication can be done over a single reference to a module.

There is also ReliableBroadcastModule, which wraps ReliableBroadcast trait. Do not modify that.

Stubborn broadcast trait
As with reliable broadcast, we will only touch on receive_acknowledgment here. This method must stop transmitting the message to the process specified by UUID.

Again, there is StubbornBroadcastModule (template/lib.rs) which wraps implementation of reliable broadcast into a executor module—do not modify that.

We also add to StubbornBroadcastModule a handler for Tick—this is intended for a tick registered in System and should run at fixed intervals as defined in the configuration (Configuration.retransmission_delay). The first tick must arrive within two delay units of time after scheduling at time t, and then n-th tick must arrive at time firstticktime + n * delay–we require that there is no drift in your solution. This allows for an implementation of a simple retransmission of messages which are not yet acknowledged.

Format of messages used in the system
You can find these types in template/domain.rs, and they are explained there. All of them derive Clone and Debug traits, while SystemMessageHeader derives support for hashing and equality. This enables you to use SystemMessageHeader in various efficient data structures like HashSet or HashMap.

The size of SystemMessageContent will not exceed 1024 bytes. All fields of these structures must be public (as in the provided template).

Varia
You can use logging as you see fit, just do not produce large amount of your logs at levels >= INFO when the system is working just fine, because this will impact testing.
Broadcasting is initiated by broadcast method of ReliableBroadcast. This will be done by wrapper code in tests.
