use schemas::wire_protocol::{DistroInfoV0, GpusV0, MachineConfigV0};
use tooling::config::{ApiConfig};
use tooling::ipc::*;
use tooling::query::{Maybe, Query, fail};

pub struct Context {
  api_cfg: Option<ApiConfig>,
  machine_cfg: Option<MachineConfigV0>,
}

impl Context {
  pub fn new() -> Context {
    // TODO
    let api_cfg = ApiConfig::open_default().ok();
    let machine_cfg = MachineConfigV0::query().ok();
    //let ci_cfg = CiConfigV0::query().ok();
    unimplemented!();
  }
}

pub struct RemoteConn {
}

impl RemoteConn {
  pub fn new(server: ws::Sender) -> RemoteConn {
    // TODO
    RemoteConn{}
  }
}

impl ws::Handler for RemoteConn {
  fn on_shutdown(&mut self) {
    // TODO
    eprintln!("TRACE: RemoteConn: on_shutdown");
  }

  fn on_open(&mut self, _: ws::Handshake) -> ws::Result<()> {
    // TODO
    Ok(())
  }

  fn on_message(&mut self, _: ws::Message) -> ws::Result<()> {
    // TODO
    Ok(())
  }

  fn on_close(&mut self, _: ws::CloseCode, _: &str) {
    // TODO
    eprintln!("TRACE: RemoteConn: on_close");
  }

  fn on_error(&mut self, _: ws::Error) {
    // TODO
    eprintln!("TRACE: RemoteConn: on_error");
  }

  fn on_timeout(&mut self, _: ws::util::Token) -> ws::Result<()> {
    // TODO
    Ok(())
  }
}

pub fn connect_ws() -> Maybe {
  match ws::connect("wss://guppybot.org:443", |server| RemoteConn::new(server)) {
    Err(_) => Err(fail("Failed to connect to guppybot.org")),
    Ok(_) => Ok(()),
  }
}

pub fn runloop() -> Maybe {
  // TODO: ctrl-c handler.
  let mut local_server = CtlListener::open_default()?;
  eprintln!("TRACE: guppybot: listening");
  loop {
    match local_server.accept() {
      Err(_) => continue,
      Ok(mut chan) => {
        eprintln!("TRACE: guppybot: accept conn");
        // FIXME: do not bail on send/recv errors.
        let recv_msg: Ctl2Bot = chan.recv()?;
        eprintln!("TRACE:   recv: {:?}", recv_msg);
        let send_msg = match recv_msg {
          Ctl2Bot::_QueryApiAuth => {
            // TODO
            Bot2Ctl::_QueryApiAuth(None)
          }
          Ctl2Bot::EchoApiId => {
            // TODO
            Bot2Ctl::EchoApiId(None)
          }
          Ctl2Bot::EchoMachineId => {
            // TODO
            Bot2Ctl::EchoMachineId(None)
          }
          Ctl2Bot::PrintConfig => {
            // TODO
            Bot2Ctl::PrintConfig(None)
          }
          Ctl2Bot::RegisterCiMachine{repo_url} => {
            // TODO
            Bot2Ctl::RegisterCiMachine(None)
          }
          Ctl2Bot::RegisterCiRepo{repo_url} => {
            // TODO: now we make a query with the websocket service.
            let settings_url = format!("{}/settings/hooks", repo_url);
            Bot2Ctl::RegisterCiRepo(Some(RegisterCiRepo{
              repo_url,
              webhook_payload_url: "https://guppybot.org/x/github/longshot".to_string(),
              webhook_secret: "AAAEEEIIIOOOUUU".to_string(),
              webhook_settings_url: settings_url,
            }))
          }
          Ctl2Bot::RegisterMachine => {
            // TODO
            Bot2Ctl::RegisterMachine(None)
          }
          Ctl2Bot::ReloadConfig => {
            // TODO
            let api_cfg = ApiConfig::open_default().ok();
            let machine_cfg = MachineConfigV0::query().ok();
            Bot2Ctl::ReloadConfig(None)
          }
          Ctl2Bot::UnregisterCiMachine => {
            // TODO
            Bot2Ctl::UnregisterCiMachine(None)
          }
          Ctl2Bot::UnregisterCiRepo => {
            // TODO
            Bot2Ctl::UnregisterCiRepo(None)
          }
          Ctl2Bot::UnregisterMachine => {
            // TODO
            Bot2Ctl::UnregisterMachine(None)
          }
          _ => {
            eprintln!("TRACE:   unhandled msg case, skipping");
            continue;
          }
        };
        eprintln!("TRACE:   send: {:?}", send_msg);
        chan.send(&send_msg)?;
        eprintln!("TRACE:   done");
      }
    }
  }
  Ok(())
}
