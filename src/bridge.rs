use crate::{
    config::Config,
    native::{service_codecs, Codec, Endpoint, Header, NativeContext, SerializedTopic},
    throttle::Throttle,
};
use anyhow::{anyhow, ensure, Result};
use ros_env::rcl_interfaces::{
    msg::rmw::SetParametersResult,
    srv::rmw::{SetParametersAtomically_Request, SetParametersAtomically_Response},
};
use std::{
    collections::HashMap,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
use zenoh::{
    bytes::ZBytes,
    query::{ConsolidationMode, Query, QueryTarget},
    Wait,
};

pub(crate) struct Change {
    pub config: Config,
    pub reply: mpsc::SyncSender<Result<()>>,
}
pub struct Bridge {
    changes: mpsc::Sender<Change>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Bridge {
    pub fn start(session: zenoh::Session, config: Config) -> Result<Self> {
        config.validate()?;
        let (changes, receiver) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = thread::Builder::new()
            .name("ros2rcl".into())
            .spawn(move || {
                let result = Worker::new(session, config);
                match result {
                    Ok(mut worker) => {
                        let _ = ready_tx.send(Ok(()));
                        worker.run(receiver, flag);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })?;
        match ready_rx.recv()? {
            Ok(()) => Ok(Self {
                changes,
                stop,
                thread: Some(thread),
            }),
            Err(e) => {
                let _ = thread.join();
                Err(e)
            }
        }
    }
    pub fn reconfigure(&self, config: Config) -> Result<()> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.changes.send(Change { config, reply })?;
        rx.recv()?
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
struct ExportTopic {
    endpoint: SerializedTopic,
    publisher: zenoh::pubsub::Publisher<'static>,
    prefix: Vec<u8>,
    throttle: Throttle,
}
struct ImportTopic {
    endpoint: SerializedTopic,
    rx: mpsc::Receiver<ZBytes>,
}
struct ExportService {
    endpoint: Endpoint,
    request: Arc<Codec>,
    response: Arc<Codec>,
    queryable: zenoh::query::Queryable<zenoh::handlers::FifoChannelHandler<Query>>,
    pending: HashMap<i64, (Query, Instant)>,
    timeout: Duration,
    key: String,
}
struct ImportService {
    endpoint: Endpoint,
    request: Arc<Codec>,
    response: Arc<Codec>,
    key: String,
    timeout: Duration,
    pending: HashMap<u64, Header>,
    next: u64,
    tx: mpsc::Sender<(u64, Result<ZBytes>)>,
    rx: mpsc::Receiver<(u64, Result<ZBytes>)>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
struct Routes {
    active: Arc<AtomicBool>,
    topic_exports: Vec<ExportTopic>,
    topic_imports: Vec<ImportTopic>,
    _zenoh_subscriptions: Vec<zenoh::pubsub::Subscriber<()>>,
    exports: Vec<ExportService>,
    imports: Vec<ImportService>,
    max_in_flight: usize,
}
impl Drop for Routes {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        // Drain native publishers' callbacks before unloading the dynamic plugin.
        for subscription in self._zenoh_subscriptions.drain(..) {
            let _ = subscription.undeclare().wait_callbacks().wait();
        }
        for service in &self.imports {
            for task in &service.tasks {
                task.abort();
            }
        }
        for service in &self.exports {
            for (query, _) in service.pending.values() {
                let _ = query.reply_err("route removed").wait();
            }
        }
    }
}
impl Routes {
    fn build(
        session: &zenoh::Session,
        topic_context: Rc<NativeContext>,
        context: Rc<NativeContext>,
        c: &Config,
    ) -> Result<Self> {
        let mut routes = Self {
            active: Arc::new(AtomicBool::new(false)),
            topic_exports: vec![],
            topic_imports: vec![],
            _zenoh_subscriptions: vec![],
            exports: vec![],
            imports: vec![],
            max_in_flight: c.max_in_flight,
        };
        for r in &c.publish {
            let endpoint = SerializedTopic::new(
                topic_context.clone(),
                &r.ros_type,
                &r.ros_topic,
                false,
                &r.qos,
            )?;
            let publisher = session
                .declare_publisher(r.zenoh_key())
                .wait()
                .map_err(|e| anyhow!("{e}"))?;
            routes.topic_exports.push(ExportTopic {
                prefix: endpoint.prefix().to_vec(),
                endpoint,
                publisher,
                throttle: Throttle::new(r.max_frequency),
            });
        }
        for r in &c.subscribe {
            let endpoint = SerializedTopic::new(
                topic_context.clone(),
                &r.ros_type,
                &r.ros_topic,
                true,
                &r.qos,
            )?;
            let (tx, rx) = mpsc::channel();
            let active = routes.active.clone();
            let sub = session
                .declare_subscriber(r.zenoh_key.clone())
                .callback(move |sample| {
                    if !active.load(Ordering::Acquire) {
                        return;
                    }
                    // Retain Zenoh storage; native endpoints stay on the ROS worker.
                    let _ = tx.send(sample.payload().clone());
                })
                .wait()
                .map_err(|e| anyhow!("{e}"))?;
            routes._zenoh_subscriptions.push(sub);
            routes.topic_imports.push(ImportTopic { endpoint, rx });
        }
        for r in &c.expose_services {
            let (request, response) = service_codecs(&r.ros_type)?;
            let endpoint = Endpoint::new(
                context.clone(),
                &r.ros_type,
                &r.ros_service,
                false,
                c.max_in_flight,
            )?;
            let key = r.zenoh_key();
            let queryable = session
                .declare_queryable(key.clone())
                .complete(true)
                .with(zenoh::handlers::FifoChannel::new(c.max_in_flight))
                .wait()
                .map_err(|e| anyhow!("{e}"))?;
            routes.exports.push(ExportService {
                endpoint,
                request,
                response,
                queryable,
                pending: HashMap::new(),
                timeout: Duration::from_millis(r.timeout_ms),
                key,
            });
        }
        for r in &c.query_services {
            let (request, response) = service_codecs(&r.ros_type)?;
            let endpoint = Endpoint::new(
                context.clone(),
                &r.ros_type,
                &r.ros_service,
                true,
                c.max_in_flight,
            )?;
            let (tx, rx) = mpsc::channel();
            routes.imports.push(ImportService {
                endpoint,
                request,
                response,
                key: r.zenoh_key.clone(),
                timeout: Duration::from_millis(r.timeout_ms),
                pending: HashMap::new(),
                next: 0,
                tx,
                rx,
                tasks: vec![],
            });
        }
        Ok(routes)
    }
    fn poll(&mut self, session: &zenoh::Session, runtime: &tokio::runtime::Runtime) {
        // Limit work per route so busy topics do not starve services or updates.
        for topic in &mut self.topic_exports {
            for _ in 0..64 {
                match topic.endpoint.take() {
                    Ok(Some(cdr)) => {
                        if !topic.throttle.allow(Instant::now()) {
                            continue;
                        }
                        let mut payload = Vec::with_capacity(topic.prefix.len() + cdr.len());
                        payload.extend_from_slice(&topic.prefix);
                        payload.extend_from_slice(cdr);
                        if let Err(e) = topic.publisher.put(payload).wait() {
                            tracing::warn!("topic export failed: {e}");
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!("topic export failed: {e}");
                        break;
                    }
                }
            }
        }
        for topic in &mut self.topic_imports {
            for _ in 0..64 {
                let Ok(payload) = topic.rx.try_recv() else {
                    break;
                };
                if let Err(e) = topic.endpoint.publish(&payload.to_bytes()) {
                    tracing::warn!("topic import failed: {e}");
                }
            }
        }
        for s in &mut self.exports {
            for _ in 0..self.max_in_flight {
                let query = match s.queryable.try_recv() {
                    Ok(Some(q)) => q,
                    _ => break,
                };
                let outcome = (|| -> Result<i64> {
                    ensure!(
                        s.pending.len() < self.max_in_flight,
                        "service capacity exceeded"
                    );
                    let payload = query
                        .payload()
                        .ok_or_else(|| anyhow!("service query requires payload"))?;
                    let mut msg = s.request.decode(&payload.to_bytes())?;
                    s.endpoint.request(&mut msg)
                })();
                match outcome {
                    Ok(seq) => {
                        s.pending.insert(seq, (query, Instant::now() + s.timeout));
                    }
                    Err(e) => {
                        let _ = query.reply_err(e.to_string()).wait();
                    }
                }
            }
            for _ in 0..self.max_in_flight {
                let outcome = (|| -> Result<Option<(i64, Vec<u8>)>> {
                    let mut msg = s.response.message()?;
                    s.endpoint
                        .take_response(&mut msg)?
                        .map(|seq| s.response.encode(&msg).map(|data| (seq, data)))
                        .transpose()
                })();
                match outcome {
                    Ok(Some((seq, data))) => {
                        if let Some((query, _)) = s.pending.remove(&seq) {
                            let _ = query.reply(s.key.clone(), data).wait();
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!("ROS service response failed: {e}");
                        break;
                    }
                }
            }
            let now = Instant::now();
            s.pending.retain(|_, (query, deadline)| {
                if now >= *deadline {
                    let _ = query.reply_err("ROS service timeout").wait();
                    false
                } else {
                    true
                }
            });
        }
        for s in &mut self.imports {
            s.tasks.retain(|task| !task.is_finished());
            while let Ok((id, result)) = s.rx.try_recv() {
                if let Some(header) = s.pending.remove(&id) {
                    let outcome = result
                        .and_then(|data| s.response.decode(&data.to_bytes()))
                        .and_then(|mut msg| unsafe {
                            s.endpoint.respond(&header, msg.native_mut_ptr())
                        });
                    if let Err(e) = outcome {
                        tracing::warn!(
                            "Zenoh service query failed (ROS caller will time out): {e}"
                        );
                    }
                }
            }
            for _ in 0..self.max_in_flight {
                if s.pending.len() >= self.max_in_flight {
                    break;
                }
                let outcome = (|| -> Result<Option<(Header, Vec<u8>)>> {
                    let mut msg = s.request.message()?;
                    let header = unsafe { s.endpoint.take_request(msg.native_mut_ptr()) }?;
                    header
                        .map(|h| s.request.encode(&msg).map(|data| (h, data)))
                        .transpose()
                })();
                match outcome {
                    Ok(Some((header, data))) => {
                        s.next = s.next.wrapping_add(1);
                        let id = s.next;
                        s.pending.insert(id, header);
                        let session = session.clone();
                        let key = s.key.clone();
                        let timeout = s.timeout;
                        let tx = s.tx.clone();
                        s.tasks.push(runtime.spawn(async move {
                            let result = tokio::time::timeout(timeout, async {
                                let replies = session
                                    .get(key)
                                    .payload(data)
                                    .target(QueryTarget::BestMatching)
                                    .consolidation(ConsolidationMode::None)
                                    .timeout(timeout)
                                    .await
                                    .map_err(|e| anyhow!("{e}"))?;
                                let reply = replies
                                    .recv_async()
                                    .await
                                    .map_err(|e| anyhow!("no service reply: {e}"))?;
                                match reply.result() {
                                    Ok(sample) => Ok(sample.payload().clone()),
                                    Err(e) => Err(anyhow!(
                                        "remote service error: {}",
                                        e.payload().try_to_string().unwrap_or_default()
                                    )),
                                }
                            })
                            .await
                            .unwrap_or_else(|_| Err(anyhow!("Zenoh query timeout")));
                            let _ = tx.send((id, result));
                        }));
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!("ROS service request failed: {e}");
                        break;
                    }
                }
            }
        }
    }
}

struct Worker {
    routes: Routes,
    control: Endpoint,
    context: Rc<NativeContext>,
    topic_context: Rc<NativeContext>,
    session: zenoh::Session,
    config: Config,
    runtime: tokio::runtime::Runtime,
}
impl Worker {
    fn new(session: zenoh::Session, config: Config) -> Result<Self> {
        let domain = crate::config::domain_from_env()?;
        let context = NativeContext::new(domain, &format!("{}_services", config.node_name))?;
        let topic_context = NativeContext::new(domain, &config.node_name)?;
        let control = Endpoint::new(
            context.clone(),
            "rcl_interfaces/srv/SetParametersAtomically",
            &config.config_service,
            true,
            10,
        )?;
        let routes = Routes::build(&session, topic_context.clone(), context.clone(), &config)?;
        routes.active.store(true, Ordering::Release);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        Ok(Self {
            routes,
            control,
            context,
            topic_context,
            session,
            config,
            runtime,
        })
    }
    fn apply(&mut self, config: Config) -> Result<()> {
        config.validate_update(&self.config)?;
        let next = Routes::build(
            &self.session,
            self.topic_context.clone(),
            self.context.clone(),
            &config,
        )?;
        self.routes.active.store(false, Ordering::Release);
        self.routes = next;
        self.config = config;
        self.routes.active.store(true, Ordering::Release);
        tracing::info!("ROS2RCL configuration applied");
        Ok(())
    }
    fn poll_control(&mut self) -> Result<()> {
        let mut req = SetParametersAtomically_Request::default();
        let Some(header) = unsafe {
            self.control
                .take_request((&mut req as *mut SetParametersAtomically_Request).cast())
        }?
        else {
            return Ok(());
        };
        let result = (|| -> Result<()> {
            ensure!(
                req.parameters.len() == 1,
                "expected one string parameter named config"
            );
            let parameter = &req.parameters[0];
            ensure!(
                parameter.name.to_string() == "config" && parameter.value.type_ == 4,
                "expected string parameter config"
            );
            let config: Config = serde_json::from_str(&parameter.value.string_value.to_string())?;
            self.apply(config)
        })();
        let (successful, reason) = match result {
            Ok(()) => (true, "configuration applied".to_owned()),
            Err(e) => (false, e.to_string()),
        };
        let mut response = SetParametersAtomically_Response {
            result: SetParametersResult {
                successful,
                reason: reason.into(),
            },
        };
        unsafe {
            self.control.respond(
                &header,
                (&mut response as *mut SetParametersAtomically_Response).cast(),
            )
        }
    }
    fn run(&mut self, receiver: mpsc::Receiver<Change>, stop: Arc<AtomicBool>) {
        while !stop.load(Ordering::Acquire) {
            if let Ok(change) = receiver.try_recv() {
                let result = self.apply(change.config);
                let _ = change.reply.send(result);
            }
            if let Err(e) = self.poll_control() {
                tracing::warn!("configuration service: {e}");
            }
            self.routes.poll(&self.session, &self.runtime);
            thread::sleep(Duration::from_millis(1));
        }
    }
}
