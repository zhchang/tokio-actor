# Project Tokio Actor

There are quite a few actor implementations for Rust,  for example: [actix](https://actix.rs/book/actix/sec-0-quick-start.html). Many of these implementations defines `traits` and need user to implement these `traits` one by one.

I for one, think that to some extent these are repeated works and are really boring. Thankfully, `macro` to the rescue!

What if we can do something really simple and with really little coding:

```rust
use tokio;
use tokio_actor::actors;

#[actors]
mod my_actors {
    pub enum ThingMsg {
        MsgOne { value: i32, resp: i32 },
        MsgTwo { value: f64, resp: f64 },
    }

    pub struct Thing {}

    impl Thing {
        async fn process(&mut self, msg: ThingMsg) {
            match msg {
                ThingMsg::MsgOne { resp, value } => {
                    println!("handling msg1");
                    if let Some(v) = resp {
                        let _r = v.send(value + 100);
                    }
                }
                ThingMsg::MsgTwo { resp, value } => {
                    println!("handling msg2");
                    if let Some(v) = resp {
                        let _r = v.send(value * 10.0);
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let mut a = my_actors::ActorThing::new().await;
    {
        let r = a
            .msg_one(my_actors::ThingMsg::MsgOne {
                value: 1,
                resp: None,
            })
            .await
            .unwrap();
        println!("{}", r);
    }
    {
        let r = a
            .msg_two(my_actors::ThingMsg::MsgTwo {
                value: 3.1415926,
                resp: None,
            })
            .await
            .unwrap();
        println!("{}", r);
    }
}
```

in the above example, most of the dirty/magic work was done by `macro` : `actors`

What is behind the scene?
1. it analyze the `my_actos` module, and smartly detect that struct `Thing` is an suitable `actor processor`, because it has a `impl` called `process`, and also it has a `enum` `ThingMsg` defined within the same module.
2. it generates a bunch of helper methods, in the name of variants of `enum` `ThingMsg`. In `snake_case` of course.
3. user could just call these method with following name convention: a `MsgOne`enum variant means there exist `msg_one` and `msg_one_no_wait` methods for you to call on `ActorThing` struct. 
4. `ActorThing` will perform a `tokio::spawn` that listens to an `tokio::sync::mpsc::UnboundedReceiver` for `ThingMsg` and `process` it. It will write result to `tokio::sync::oneshot` channel. Like you could have guessed, `msg_one_no_wait` simply does not care to wait for the result to come back.

let's look at the generated `tokenstream` for `mod` `my_actors` in this example:
```rust
mod my_actors {
    pub enum ThingMsg {
        MsgOne {
            value: i32,
            resp: Option<tokio::sync::oneshot::Sender<i32>>,
        },
        MsgTwo {
            value: f64,
            resp: Option<tokio::sync::oneshot::Sender<f64>>,
        },
    }
    pub struct Thing {
        receiver: tokio::sync::mpsc::UnboundedReceiver<ThingMsg>,
    }
    impl Thing {
        async fn process(&mut self, msg: ThingMsg) {
            match msg {
                ThingMsg::MsgOne { resp, value } => {
                    println!("handling msg1");
                    if let Some(v) = resp {
                        let _r = v.send(value + 100);
                    }
                }
                ThingMsg::MsgTwo { resp, value } => {
                    println!("handling msg2");
                    if let Some(v) = resp {
                        let _r = v.send(value * 10.0);
                    }
                }
            }
        }
    }
    pub struct ActorThing {
        sender: tokio::sync::mpsc::UnboundedSender<ThingMsg>,
    }
    impl ActorThing {
        pub async fn new() -> Self {
            let (s, r) = tokio::sync::mpsc::unbounded_channel();
            let mut a = Thing::new(r);
            tokio::spawn(async move {
                a.run().await;
            });
            return Self { sender: s };
        }
    }
    impl Thing {
        fn new(r: tokio::sync::mpsc::UnboundedReceiver<ThingMsg>) -> Self {
            return Self { receiver: r };
        }
        async fn run(&mut self) {
            while let Some(msg) = self.receiver.recv().await {
                self.process(msg).await;
            }
        }
    }
    impl ActorThing {
        pub async fn msg_one(&mut self, mut msg: ThingMsg) -> Result<i32, &'static str> {
            match msg {
                ThingMsg::MsgOne { ref mut resp, .. } => {
                    let (mut s, mut r) = tokio::sync::oneshot::channel();
                    *resp = Some(s);
                    self.sender.send(msg).map_err(|_e| {
                        return "send failed";
                    })?;
                    match r.await {
                        Ok(v) => {
                            return Ok(v);
                        }
                        _ => {
                            return Err("mailbox closed");
                        }
                    };
                }
                _ => {
                    return Err("invalid msg type");
                }
            };
        }
    }
    impl ActorThing {
        pub async fn msg_one_no_wait(&mut self, mut msg: ThingMsg) -> Result<(), &'static str> {
            match msg {
                ThingMsg::MsgOne { .. } => {
                    self.sender.send(msg).map_err(|_e| {
                        return "send failed";
                    })?;
                    return Ok(());
                }
                _ => {
                    return Err("invalid msg type");
                }
            };
        }
    }
    impl ActorThing {
        pub async fn msg_two(&mut self, mut msg: ThingMsg) -> Result<f64, &'static str> {
            match msg {
                ThingMsg::MsgTwo { ref mut resp, .. } => {
                    let (mut s, mut r) = tokio::sync::oneshot::channel();
                    *resp = Some(s);
                    self.sender.send(msg).map_err(|_e| {
                        return "send failed";
                    })?;
                    match r.await {
                        Ok(v) => {
                            return Ok(v);
                        }
                        _ => {
                            return Err("mailbox closed");
                        }
                    };
                }
                _ => {
                    return Err("invalid msg type");
                }
            };
        }
    }
    impl ActorThing {
        pub async fn msg_two_no_wait(&mut self, mut msg: ThingMsg) -> Result<(), &'static str> {
            match msg {
                ThingMsg::MsgTwo { .. } => {
                    self.sender.send(msg).map_err(|_e| {
                        return "send failed";
                    })?;
                    return Ok(());
                }
                _ => {
                    return Err("invalid msg type");
                }
            };
        }
    }
}
```

FAQ:
- what's next?
	- will allow multiple senders and multiple actors handling them 
- Do we have to define a `mod` for actors?
	- yes, for now and for foreseeable future. Because I need to analyze `struct`, `enum`, `impl` together, the best way to organize them in `Rust` is `mod`.
- So what is the requirement for an `Actor` to be generated by the macro?
	- You need a `struct` called XXX and a `enum` called XXXMsg.
	- the `enum` XXXMsg has to have at least 1 `variant`, the `variant` needs to have `named fields` like shown in the example, and we need one specific `named field` called `resp`. the `type` of this `resp` `named field` determine `msg function` return type. 
	- The XXX `struct` needs to implement a `process` method, that takes `msg:XXXMsg` as an input parameter. This is where the actual message handling happens.
