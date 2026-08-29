//! `SCM_RIGHTS` on a Unix stream: send bytes with one descriptor riding on
//! them, receive bytes collecting one if it arrived.
//!
//! The descriptor is attached to the frame's length prefix, so a receiver
//! that reads any of the prefix gets it, and a frame is never split from its
//! descriptor by a short read of the body.

use std::io;
use std::mem;
use std::os::fd::{BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::os::fd::AsRawFd;

// `iov` is read through the raw pointer in `msg`, which the compiler cannot see.
#[allow(unused_assignments)]
pub fn send_with_fd(sock: &UnixStream, bytes: &[u8], fd: Option<BorrowedFd<'_>>) -> io::Result<()> {
    let mut iov = libc::iovec { iov_base: bytes.as_ptr() as *mut libc::c_void, iov_len: bytes.len() };
    // SAFETY: zeroed msghdr/cmsg buffers filled in below with the libc macros,
    // all pointing at memory that outlives the sendmsg call.
    unsafe {
        let mut msg: libc::msghdr = mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        let space = libc::CMSG_SPACE(mem::size_of::<RawFd>() as u32) as usize;
        let mut cbuf = vec![0u8; space];
        if let Some(fd) = fd {
            msg.msg_control = cbuf.as_mut_ptr() as *mut libc::c_void;
            msg.msg_controllen = space as _;
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(mem::size_of::<RawFd>() as u32) as _;
            std::ptr::write_unaligned(libc::CMSG_DATA(cmsg) as *mut RawFd, fd.as_raw_fd());
        }
        let mut sent = 0usize;
        while sent < bytes.len() {
            let n = libc::sendmsg(sock.as_raw_fd(), &msg, libc::MSG_NOSIGNAL);
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            sent += n as usize;
            // The descriptor went with the first bytes; the rest is plain.
            msg.msg_control = std::ptr::null_mut();
            msg.msg_controllen = 0;
            iov.iov_base = bytes[sent..].as_ptr() as *mut libc::c_void;
            iov.iov_len = bytes.len() - sent;
        }
    }
    Ok(())
}

/// Fill `buf` completely, collecting a descriptor if one arrives with any
/// of it. EOF before the buffer is full is `UnexpectedEof`.
pub fn recv_exact_with_fd(sock: &UnixStream, buf: &mut [u8]) -> io::Result<Option<OwnedFd>> {
    let mut received: Option<OwnedFd> = None;
    let mut got = 0usize;
    while got < buf.len() {
        let mut iov = libc::iovec { iov_base: buf[got..].as_mut_ptr() as *mut libc::c_void, iov_len: buf.len() - got };
        // SAFETY: as in send_with_fd; the cmsg buffer is ours and sized by
        // the libc macro.
        let n = unsafe {
            let mut msg: libc::msghdr = mem::zeroed();
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            let space = libc::CMSG_SPACE(mem::size_of::<RawFd>() as u32) as usize;
            let mut cbuf = vec![0u8; space];
            msg.msg_control = cbuf.as_mut_ptr() as *mut libc::c_void;
            msg.msg_controllen = space as _;
            let n = libc::recvmsg(sock.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC);
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
            while !cmsg.is_null() {
                if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                    let fd = std::ptr::read_unaligned(libc::CMSG_DATA(cmsg) as *const RawFd);
                    let owned = OwnedFd::from_raw_fd(fd);
                    // One descriptor per frame is the contract; a second
                    // is closed rather than leaked.
                    if received.is_none() {
                        received = Some(owned);
                    }
                }
                cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
            }
            n as usize
        };
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "peer closed"));
        }
        got += n;
    }
    Ok(received)
}
