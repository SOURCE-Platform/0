use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::collections::HashMap;

/// Advertise `_source-mobile._tcp` while the HTTPS server is up.
/// Presence of this record is the phone's "is Source running?" signal.
pub struct MobileAdvertiser {
    daemon: ServiceDaemon,
    service_fullname: Option<String>,
}

impl MobileAdvertiser {
    pub fn new() -> Result<Self, String> {
        let daemon =
            ServiceDaemon::new().map_err(|error| format!("Failed to start mDNS: {error}"))?;
        // The daemon rescans every network interface every 5 s by default to
        // notice IP changes. With a phone plugged in there are ~20 interfaces,
        // and that scan showed up in Source's CPU profile. Checking once a
        // minute is plenty for noticing the Mac has moved networks.
        let _ = daemon.set_ip_check_interval(60);
        Ok(Self {
            daemon,
            service_fullname: None,
        })
    }

    pub fn advertise(
        &mut self,
        port: u16,
        fingerprint: &str,
        device_name: &str,
    ) -> Result<(), String> {
        self.unadvertise();
        let hostname = hostname::get()
            .ok()
            .and_then(|name| name.into_string().ok())
            .unwrap_or_else(|| "source".to_string());
        let mut properties = HashMap::new();
        properties.insert("port".to_string(), port.to_string());
        properties.insert("fp".to_string(), fingerprint.to_string());
        properties.insert("v".to_string(), "1".to_string());
        properties.insert("name".to_string(), device_name.to_string());
        let service = ServiceInfo::new(
            "_source-mobile._tcp",
            &hostname,
            &format!("{hostname}._source-mobile._tcp.local."),
            "",
            port,
            Some(properties),
        )
        .map_err(|error| format!("Failed to build mDNS record: {error}"))?;
        self.service_fullname = Some(service.get_fullname().to_string());
        self.daemon
            .register(service)
            .map_err(|error| format!("Failed to advertise mobile service: {error}"))?;
        println!("Source Mobile advertising on port {port}");
        Ok(())
    }

    pub fn unadvertise(&mut self) {
        if let Some(fullname) = self.service_fullname.take() {
            let _ = self.daemon.unregister(&fullname);
        }
    }
}

impl Drop for MobileAdvertiser {
    fn drop(&mut self) {
        self.unadvertise();
    }
}
