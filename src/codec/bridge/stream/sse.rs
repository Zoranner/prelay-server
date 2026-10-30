pub(crate) fn drain_lines(line_buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();

    while let Some(newline) = line_buffer.iter().position(|byte| *byte == b'\n') {
        let mut line = line_buffer.drain(..=newline).collect::<Vec<_>>();
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        lines.push(line);
    }

    lines
}
