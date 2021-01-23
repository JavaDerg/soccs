use std::time::Duration;
use flume::TryRecvError;

struct Writer {
    handle: Option<std::thread::JoinHandle<()>>,
    sender: Option<flume::Sender<(u32, String)>>,
}

impl Writer {
    pub fn write(&self, str: String) {
        self.write_col(term::color::BRIGHT_WHITE, str);
    }

    pub fn write_col(&self, col: u32, str: String) {
        self.sender.as_ref().unwrap().send((col, str)).unwrap();
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        drop(self.sender.take());
        self.handle.take().unwrap().join().unwrap();
    }
}

fn main() {
    let mut term = term::stderr().unwrap();

    let address = std::env::args().skip(1).next();
    if address.is_none() {
        term.fg(term::color::RED).unwrap();
        term.write("No Address provided\n".as_bytes()).unwrap();
        std::process::exit(1);
    }
    let address = address.unwrap();

    let (writer, rx) = flume::unbounded::<(u32, String)>();
    let writer_thread = std::thread::spawn(move || {
        for (color, msg) in rx.into_iter() {
            term.fg(color).unwrap();
            term.write(msg.as_bytes()).unwrap();
        }
    });
    let writer = Writer {
        handle: Some(writer_thread),
        sender: Some(writer),
    };
    writer.write(format!("Connecting to {}...\n", address));

    let mut builder = match websocket::client::ClientBuilder::new(&address) {
        Ok(builder) => builder,
        Err(err) => {
            writer.write_col(term::color::RED, format!("Failed to create ws builder: {:?}\n", err));
            return;
        }
    };

    let mut conn = match builder.connect(None) {
        Ok(conn) => conn,
        Err(err) => {
            writer.write_col(term::color::RED, format!("Failed to connect to ws: {:?}\n", err));
            return;
        }
    };

    writer.write(String::from("Connected!\n"));

    let (tx, rx) = flume::unbounded();
    let wt = writer.sender.as_ref().unwrap().clone();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut str = String::with_capacity(1024);
        match stdin.read_line(&mut str) {
            Ok(read) => {
                if read == 0 {
                    return;
                }
                tx.send(str.clone()).unwrap();
                str.clear();
            }
            Err(err) => {
                wt.send((term::color::RED, format!("Unable to read line: {}", err))).unwrap();
                return;
            }
        }
    });


    conn.set_nonblocking(true).unwrap();
    conn.set_nodelay(true).unwrap();

    let mut str = String::with_capacity(1024);
    loop {
        let mut work = false;
        match conn.reader_mut().read_to_string(&mut str) {
            Ok(read) => {
                if read == 0 {
                    writer.write_col(term::color::CYAN, String::from("EOF\n"));
                    std::thread::sleep(Duration::new(1, 0)); // Ugly solution, but it works for now FIXME
                    std::process::exit(0);
                }
                writer.write_col(term::color::BLUE, String::from("< "));
                writer.write(format!("{}\n", str));
                str.clear();
                work = true;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(err) => {
                writer.write_col(term::color::RED, format!("{:?}", err));
                std::thread::sleep(Duration::new(1, 0)); // Ugly solution, but it works for now FIXME
                std::process::exit(0);
            }
        }

        let mut set = false;
        loop {
            match rx.try_recv() {
                Ok(msg) => {
                    if !set {
                        conn.set_nonblocking(false).unwrap();
                        set = true;
                    }
                    if let Err(err) = conn.writer_mut().write_all(msg.as_bytes()) {
                        writer.write_col(term::color::RED, format!("{:?}", err));
                        std::thread::sleep(Duration::new(1, 0)); // Ugly solution, but it works for now FIXME
                        std::process::exit(0);
                    }
                }
                Err(TryRecvError::Disconnected) => std::process::exit(0),
                _ => break,
            }
            if set {
                conn.set_nonblocking(true).unwrap();
                work = true;
            }
        }

        if !work {
            std::thread::yield_now();
        }
    }
}
