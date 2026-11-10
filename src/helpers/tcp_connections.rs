//! Established TCP connections on local ports, read from `/proc/net/tcp*`.
//!
//! The kernel's connection tables are readable by any user, so this works for
//! sockets owned by another process.

const TABLES: [&str; 2] = ["/proc/net/tcp", "/proc/net/tcp6"];

/// Whether `table` (the text of a `/proc/net/tcp` or `/proc/net/tcp6` file)
/// lists an established connection whose local port is one of `ports`.
pub fn table_has_established(table: &str, ports: &[u16]) -> bool {
    const ESTABLISHED: &str = "01";
    table.lines().skip(1).any(|line| {
        let mut fields = line.split_whitespace();
        let local = fields.nth(1);
        let state = fields.nth(1);
        let port = local
            .and_then(|l| l.rsplit_once(':'))
            .and_then(|(_, p)| u16::from_str_radix(p, 16).ok());
        state == Some(ESTABLISHED) && port.is_some_and(|p| ports.contains(&p))
    })
}

/// Whether any established TCP connection, IPv4 or IPv6, has one of `ports`
/// as its local port. Unreadable tables count as no connection.
pub fn has_established_on_local_ports(ports: &[u16]) -> bool {
    if ports.is_empty() {
        return false;
    }
    TABLES.iter().any(|path| {
        std::fs::read_to_string(path)
            .map(|table| table_has_established(&table, ports))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";

    fn table(rows: &[&str]) -> String {
        std::iter::once(HEADER).chain(rows.iter().copied()).collect::<Vec<_>>().join("\n")
    }

    // 7000 = 0x1B58, 5000 = 0x1388
    const V6_ESTABLISHED_7000: &str = "   0: 000080FE00000000FA3AF6AC1A0F7CFE:1B58 000080FE00000000FB9AFB0803000000:F12E 01 00000000:00000000 00:00000000 00000000  1000        0 123 1 0000000000000000 20 4 30 10 -1";
    const V6_LISTEN_7000: &str = "   1: 00000000000000000000000000000000:1B58 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 124 1 0000000000000000 100 0 0 10 0";
    const V4_ESTABLISHED_5000: &str = "   2: 0B01A8C0:1388 0C01A8C0:D431 01 00000000:00000000 00:00000000 00000000  1000        0 125 1 0000000000000000 20 4 30 10 -1";
    const V4_ESTABLISHED_REMOTE_7000: &str = "   3: 0B01A8C0:D431 0C01A8C0:1B58 01 00000000:00000000 00:00000000 00000000  1000        0 126 1 0000000000000000 20 4 30 10 -1";
    const V4_TIME_WAIT_7000: &str = "   4: 0B01A8C0:1B58 0C01A8C0:D432 06 00000000:00000000 00:00000000 00000000     0        0 0 3 0000000000000000";

    #[test]
    fn established_ipv6_on_port_is_found() {
        assert!(table_has_established(&table(&[V6_LISTEN_7000, V6_ESTABLISHED_7000]), &[7000]));
    }

    #[test]
    fn established_ipv4_on_second_port_is_found() {
        assert!(table_has_established(&table(&[V4_ESTABLISHED_5000]), &[7000, 5000]));
    }

    #[test]
    fn listening_socket_alone_is_not_a_connection() {
        assert!(!table_has_established(&table(&[V6_LISTEN_7000]), &[7000]));
    }

    #[test]
    fn remote_port_does_not_count() {
        assert!(!table_has_established(&table(&[V4_ESTABLISHED_REMOTE_7000]), &[7000]));
    }

    #[test]
    fn closing_connection_does_not_count() {
        assert!(!table_has_established(&table(&[V4_TIME_WAIT_7000]), &[7000]));
    }

    #[test]
    fn other_port_does_not_count() {
        assert!(!table_has_established(&table(&[V6_ESTABLISHED_7000]), &[5000]));
    }

    #[test]
    fn garbage_and_empty_tables_are_no_connection() {
        assert!(!table_has_established("", &[7000]));
        assert!(!table_has_established("not a table\n  x: y z", &[7000]));
    }

    #[test]
    fn no_ports_means_no_connection() {
        assert!(!has_established_on_local_ports(&[]));
    }
}
