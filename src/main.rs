use std::env;
use std::net;
use std::process;
use std::thread::*;
use std::time::Duration;

use crossbeam_channel as cbc;
use network_rust::udpnet;

// Data types to be sent on the network must derive traits for serialization
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CustomDataType {
    message: String,
    iteration: u64,
}

fn main() -> std::io::Result<()> {
    // Genreate id: either from command line, or a default rust@ip#pid
    let args: Vec<String> = env::args().collect();
    let id = if args.len() > 1 {
        args[1].clone()
    } else {
        let local_ip = net::TcpStream::connect("8.8.8.8:53")
            .unwrap()
            .local_addr()
            .unwrap()
            .ip();
        format!("rust@{}#{}", local_ip, process::id())
    };

    let msg_port = 19735;
    let peer_port = 19738;

    // send a message here if we are ever disconnected from the network
    let (disconnected_tx, disconnected_rx) = cbc::unbounded::<()>();

    // The sender for peer discovery
    let (peer_tx_enable_tx, peer_tx_enable_rx) = cbc::unbounded::<bool>();
    let _handler = {
        let id = id.clone();
        let disconnected_tx = disconnected_tx.clone();
        spawn(move || {
            if udpnet::peers::tx(peer_port, id, peer_tx_enable_rx).is_err() {
                disconnected_tx.send(()).unwrap();
            }
        })
    };

    // (periodically disable/enable the peer broadcast, to provoke new peer / peer loss messages)
    spawn(move || loop {
        sleep(Duration::new(6, 0));
        peer_tx_enable_tx.send(false).unwrap();
        sleep(Duration::new(3, 0));
        peer_tx_enable_tx.send(true).unwrap();
    });

    // The receiver for peer discovery updates
    let (peer_update_tx, peer_update_rx) = cbc::unbounded::<udpnet::peers::PeerUpdate>();
    {
        let disconnected_tx = disconnected_tx.clone();
        spawn(move || {
            if udpnet::peers::rx(peer_port, peer_update_tx).is_err() {
                disconnected_tx.send(()).unwrap();
            }
        });
    }

    // Periodically produce a custom data message
    let (custom_data_send_tx, custom_data_send_rx) = cbc::unbounded::<CustomDataType>();
    {
        spawn(move || {
            let mut cd = CustomDataType {
                message: format!("Hello from node {}", id),
                iteration: 0,
            };
            loop {
                custom_data_send_tx.send(cd.clone()).unwrap();
                cd.iteration += 1;
                sleep(Duration::new(1, 0));
            }
        });
    }
    // The sender for our custom data
    {
        let disconnected_tx = disconnected_tx.clone();
        spawn(move || {
            if udpnet::bcast::tx(msg_port, custom_data_send_rx).is_err() {
                disconnected_tx.send(()).unwrap();
            }
        });
    }
    // The receiver for our custom data
    let (custom_data_recv_tx, custom_data_recv_rx) = cbc::unbounded::<CustomDataType>();
    spawn(move || {
        if udpnet::bcast::rx(msg_port, custom_data_recv_tx).is_err() {
            disconnected_tx.send(()).unwrap();
        }
    });
    if disconnected_rx.recv_timeout(Duration::from_secs(1)).is_ok() {
        panic!("Unable to connect to network");
    }

    // main body: receive peer updates and data from the network
    loop {
        cbc::select! {
            recv(peer_update_rx) -> a => {
                let update = a.unwrap();
                println!("{:#?}", update);
            }
            recv(custom_data_recv_rx) -> a => {
                let cd = a.unwrap();
                println!("{:#?}", cd);
            }
        }
    }
}
