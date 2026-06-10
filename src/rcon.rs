use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub struct RconClient {
    stream: TcpStream,
    request_id: i32,
}

impl RconClient {
    pub fn connect(host: &str, port: u16) -> Result<Self> {
        let stream = TcpStream::connect((host, port))
            .with_context(|| format!("Failed to connect to RCON at {host}:{port}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .context("Failed to set RCON read timeout")?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .context("Failed to set RCON write timeout")?;
        Ok(Self {
            stream,
            request_id: 0,
        })
    }

    pub fn login(&mut self, password: &str) -> Result<()> {
        self.request_id += 1;
        let request_id = self.request_id;
        self.send_packet(request_id, 3, password)?;
        let packet = self.read_packet()?;

        if packet.request_id == -1 || packet.request_id != request_id {
            bail!("RCON login failed");
        }

        Ok(())
    }

    pub fn command(&mut self, command: &str) -> Result<String> {
        self.request_id += 1;
        let request_id = self.request_id;
        self.send_packet(request_id, 2, command)?;
        let packet = self.read_packet()?;

        if packet.request_id != request_id {
            bail!("Unexpected RCON response id {}", packet.request_id);
        }

        Ok(packet.body)
    }

    fn send_packet(&mut self, request_id: i32, packet_type: i32, body: &str) -> Result<()> {
        let body_bytes = body.as_bytes();
        let size = 4 + 4 + body_bytes.len() as i32 + 2;

        let mut buffer = Vec::with_capacity(size as usize + 4);
        buffer.extend(size.to_le_bytes());
        buffer.extend(request_id.to_le_bytes());
        buffer.extend(packet_type.to_le_bytes());
        buffer.extend(body_bytes);
        buffer.push(0);
        buffer.push(0);

        self.stream
            .write_all(&buffer)
            .context("Failed to write RCON packet")
    }

    fn read_packet(&mut self) -> Result<RconPacket> {
        let mut size_bytes = [0u8; 4];
        self.stream
            .read_exact(&mut size_bytes)
            .context("Failed to read RCON packet size")?;

        let size = i32::from_le_bytes(size_bytes);
        if size < 10 {
            bail!("Invalid RCON packet size {size}");
        }

        let mut payload = vec![0u8; size as usize];
        self.stream
            .read_exact(&mut payload)
            .context("Failed to read RCON packet payload")?;

        let request_id = i32::from_le_bytes(
            payload[0..4]
                .try_into()
                .context("Invalid RCON request id field")?,
        );
        let packet_type = i32::from_le_bytes(
            payload[4..8]
                .try_into()
                .context("Invalid RCON packet type field")?,
        );
        let body_end = payload.len().saturating_sub(2);
        let body = String::from_utf8_lossy(&payload[8..body_end]).to_string();

        Ok(RconPacket {
            request_id,
            #[allow(dead_code)]
            packet_type,
            body,
        })
    }
}

struct RconPacket {
    request_id: i32,

    #[allow(dead_code)]
    packet_type: i32,

    body: String,
}
