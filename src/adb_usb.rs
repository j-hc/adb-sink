use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::hazmat::PrehashSigner;
use rsa::signature::SignatureEncoding;
use rsa::RsaPrivateKey;
use rusb::{DeviceHandle, GlobalContext};
use sha1::digest::core_api::CoreWrapper;
use sha1::Sha1Core;
use std::ffi::CStr;
use std::fmt::Debug;
use std::time::Duration;

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
enum Cmd {
    CNXN = 1314410051,
    AUTH = 1213486401,
    CLSE = 1163086915,
    OKAY = 1497451343,
    OPEN = 1313165391,
    SYNC = 1129208147,
    WRTE = 1163154007,
}
impl TryFrom<u32> for Cmd {
    type Error = ();
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            v if Self::CNXN as u32 == v => Ok(Self::CNXN),
            v if Self::AUTH as u32 == v => Ok(Self::AUTH),
            v if Self::CLSE as u32 == v => Ok(Self::CLSE),
            v if Self::OKAY as u32 == v => Ok(Self::OKAY),
            v if Self::OPEN as u32 == v => Ok(Self::OPEN),
            v if Self::SYNC as u32 == v => Ok(Self::SYNC),
            v if Self::WRTE as u32 == v => Ok(Self::WRTE),
            _ => Err(()),
        }
    }
}

const VERSION: u32 = 0x01000000;
const MAX_ADB_DATA: u32 = 1024 * 1024;
const MSG_SIZE: usize = 4 * 6;

const AUTH_SIG: u32 = 2;
const AUTH_TOKEN: u32 = 1;

const TIMEOUT: Duration = Duration::from_secs(1);

fn main() {
    let priv_key_path = "adbkey";
    let priv_key_str = std::fs::read_to_string(priv_key_path).unwrap();
    let signer = get_signer(&priv_key_str);

    let mut device = connect_device().unwrap();
    device.flush().unwrap();
    let r = device.connect(signer, c"host::jhc-abra").unwrap();
    dbg!(ByteStr(&r));

    device.msg_open(b"shell:echo TEST2\0").unwrap();
}

struct ByteStr<'a>(&'a [u8]);
impl<'a> Debug for ByteStr<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in self.0 {
            match b {
                b if *b == 0x20 => write!(f, " ")?,
                b if b.is_ascii_graphic() => write!(f, "{}", *b as char)?,
                _ => write!(f, "\\x{:02x}", b)?,
            }
        }
        Ok(())
    }
}

type AdbSigner = rsa::pkcs1v15::SigningKey<CoreWrapper<Sha1Core>>;

fn get_signer(priv_key_str: &str) -> AdbSigner {
    let priv_key = RsaPrivateKey::from_pkcs8_pem(priv_key_str).unwrap();
    SigningKey::<sha1::Sha1>::new(priv_key)
}

#[derive(Debug)]
struct AdbDevice {
    handle: DeviceHandle<GlobalContext>,
    read_endpoint: u8,
    write_endpoint: u8,
    remote_id: u32,
    local_id: u32,
}
impl AdbDevice {
    fn msg_open(&mut self, data: &[u8]) -> rusb::Result<()> {
        self.local_id = 1;
        self.send(AdbMsg::pack(Cmd::OPEN, self.local_id, 0, data), TIMEOUT)?;
        let (msg, _data) = self.read_packet(TIMEOUT)?;
        assert!(msg.cmd == Cmd::OKAY);
        self.remote_id = msg.arg0;

        let r = &self.read_to_end().collect::<Vec<_>>()[0];
        dbg!(msg, ByteStr(r));
        Ok(())
    }

    fn read_to_end(&mut self) -> impl Iterator<Item = Vec<u8>> + '_ {
        std::iter::from_fn(move || {
            let (msg, data) = self.read_packet(TIMEOUT).unwrap();
            match msg.cmd {
                Cmd::CLSE => {
                    self.send(
                        AdbMsg::pack(Cmd::CLSE, self.local_id, self.remote_id, &[]),
                        TIMEOUT,
                    )
                    .unwrap();
                    None
                }
                Cmd::WRTE => {
                    self.msg_okay().unwrap();
                    Some(data)
                }
                _ => panic!("wrong packet: '{:?}'", msg),
            }
        })
    }

    fn msg_okay(&mut self) -> rusb::Result<()> {
        self.send(
            AdbMsg::pack(Cmd::OKAY, self.local_id, self.remote_id, &[]),
            TIMEOUT,
        )
    }

    fn connect(
        &mut self,
        signer: rsa::pkcs1v15::SigningKey<CoreWrapper<Sha1Core>>,
        host: &CStr,
    ) -> rusb::Result<Vec<u8>> {
        self.send(
            AdbMsg::pack(Cmd::CNXN, VERSION, MAX_ADB_DATA, host.to_bytes()),
            TIMEOUT,
        )?;
        let (auth_msg, banner2) = self.read_packet(TIMEOUT)?;
        match auth_msg.cmd {
            Cmd::CNXN => todo!("already AUTH!"),
            Cmd::AUTH => (),
            c => panic!("ERROR: {c:?}"),
        }
        if auth_msg.arg0 != AUTH_TOKEN {
            panic!("ERROR: wront auth token {}", auth_msg.arg0);
        }
        let signed_auth = signer.sign_prehash(&banner2).expect("prehashed").to_bytes();
        self.send(AdbMsg::pack(Cmd::AUTH, AUTH_SIG, 0, &signed_auth), TIMEOUT)
            .unwrap();
        let (msg, data) = self.read_packet(TIMEOUT).unwrap();
        match msg.cmd {
            Cmd::CNXN => (),
            Cmd::AUTH => panic!("unexpected auth"),
            _ => panic!("couldnt auth"),
        }
        Ok(data)
    }

    fn flush(&mut self) -> rusb::Result<()> {
        let mut buf = [0u8; 64];
        loop {
            match self
                .handle
                .read_bulk(self.read_endpoint, &mut buf, Duration::from_millis(900))
            {
                Err(e @ rusb::Error::Overflow) => return Err(e),
                Err(rusb::Error::Timeout) => return Ok(()),
                Ok(0) => return Ok(()),
                _ => (),
            }
        }
    }

    fn send(&mut self, msg: AdbMsg, timeout: Duration) -> rusb::Result<()> {
        println!("send1: {:?}", ByteStr(&msg.packed));
        let r = self
            .handle
            .write_bulk(self.write_endpoint, &msg.packed, timeout)?;
        assert_eq!(r, MSG_SIZE);
        if !msg.data.is_empty() {
            println!("send2: {:?}", ByteStr(msg.data));
            let r = self
                .handle
                .write_bulk(self.write_endpoint, msg.data, timeout)?;
            assert_eq!(r, msg.data.len());
        }

        Ok(())
    }

    fn read_packet(&mut self, timeout: Duration) -> rusb::Result<(UnpackedAdbMsg, Vec<u8>)> {
        let mut buf = [0u8; MSG_SIZE];
        self.read(&mut buf, timeout)?;
        let msg = UnpackedAdbMsg::unpack(&buf);
        if msg.data_len == 0 {
            return Ok((msg, Vec::new()));
        }

        let mut data = vec![0u8; msg.data_len as usize];
        self.read(&mut data, timeout)?;
        assert!(msg.checksum == checksum(&data));
        Ok((msg, data))
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> rusb::Result<()> {
        let mut read = 0;
        while read < buf.len() {
            let r = self
                .handle
                .read_bulk(self.read_endpoint, &mut buf[read..], timeout)?;
            read += r;
        }
        println!("read: {:?}", ByteStr(buf));
        Ok(())
    }
}

struct AdbMsg<'d> {
    packed: [u8; MSG_SIZE],
    data: &'d [u8],
}
impl<'d> AdbMsg<'d> {
    fn pack(cmd: Cmd, arg0: u32, arg1: u32, data: &'d [u8]) -> Self {
        let mut buf = [0u8; MSG_SIZE];
        let mut packed = Packer::new(&mut buf);
        let cmd = cmd as u32;
        let magic = cmd ^ 0xFFFFFFFF;
        packed.write_u32(cmd);
        packed.write_u32(arg0);
        packed.write_u32(arg1);
        packed.write_u32(data.len() as u32);
        packed.write_u32(checksum(data));
        packed.write_u32(magic);
        Self { packed: buf, data }
    }
}

#[derive(Debug)]
struct UnpackedAdbMsg {
    cmd: Cmd,
    arg0: u32,
    arg1: u32,
    data_len: u32,
    checksum: u32,
}
impl UnpackedAdbMsg {
    fn unpack(buf: &[u8]) -> Self {
        let mut packed = UnPacker::new(buf);
        let cmd: Cmd = packed.read_u32().try_into().expect("wrong cmd");
        let arg0 = packed.read_u32();
        let arg1 = packed.read_u32();
        let data_len = packed.read_u32();
        let checksum = packed.read_u32();
        Self {
            cmd,
            arg0,
            arg1,
            data_len,
            checksum,
        }
    }
}

struct UnPacker<'d> {
    inner: &'d [u8],
    cur: usize,
}
impl<'d> UnPacker<'d> {
    fn new(inner: &'d [u8]) -> Self {
        Self { inner, cur: 0 }
    }
    fn read_u32(&mut self) -> u32 {
        let b = &self.inner[self.cur..self.cur + 4];
        self.cur += 4;
        let b: &[u8; 4] = unsafe { b.try_into().unwrap_unchecked() };
        u32::from_le_bytes(*b)
    }
}

struct Packer<'d> {
    inner: &'d mut [u8],
    cur: usize,
}

impl<'d> Packer<'d> {
    fn new(inner: &'d mut [u8]) -> Self {
        Self { inner, cur: 0 }
    }
    fn write_u32(&mut self, b: u32) {
        self.inner[self.cur..self.cur + 4].copy_from_slice(&b.to_le_bytes());
        self.cur += 4;
    }
}

fn checksum(data: &[u8]) -> u32 {
    data.iter().map(|&b| b as u32).sum::<u32>()
}

impl<'d> Debug for AdbMsg<'d> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdbMsg")
            .field("packed", &ByteStr(&self.packed))
            .field("data", &ByteStr(self.data))
            .finish()
    }
}

fn connect_device() -> Option<AdbDevice> {
    for device in rusb::devices().unwrap().iter() {
        let mut handle = device.open().unwrap();

        let mut read_endpoint = 0;
        let mut write_endpoint = 0;
        for interface in device.active_config_descriptor().unwrap().interfaces() {
            for idesc in interface.descriptors() {
                let i = (
                    idesc.class_code(),
                    idesc.sub_class_code(),
                    idesc.protocol_code(),
                );
                let interface_number = match i {
                    (255, 66, 1) => interface.number(),
                    _ => continue,
                };
                for endpoint in idesc.endpoint_descriptors() {
                    match endpoint.direction() {
                        rusb::Direction::In => read_endpoint = endpoint.address(),
                        rusb::Direction::Out => write_endpoint = endpoint.address(),
                    }
                }
                assert!(read_endpoint != 0);
                assert!(write_endpoint != 0);

                handle.claim_interface(interface_number).unwrap();
                return Some(AdbDevice {
                    handle,
                    read_endpoint,
                    write_endpoint,
                    remote_id: 0,
                    local_id: 0,
                });
            }
        }
    }

    None
}
