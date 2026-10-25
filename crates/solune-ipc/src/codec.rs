//! Framing: uma mensagem JSON por linha, terminada em `\n`.

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Tamanho máximo de uma linha. Nenhuma mensagem do protocolo chega perto;
/// o limite só impede que um par com defeito faça o outro alocar sem fim.
pub const MAX_LINE: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("falha de E/S no socket: {0}")]
    Io(#[from] std::io::Error),
    #[error("mensagem maior que {MAX_LINE} bytes")]
    TooLong,
    #[error("mensagem inválida: {0}")]
    Invalid(#[from] serde_json::Error),
}

/// Lê a próxima mensagem. `None` quando o outro lado fechou a conexão.
pub async fn read_message<T, R>(reader: &mut R) -> Result<Option<T>, CodecError>
where
    T: DeserializeOwned,
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();
    let read = (&mut *reader)
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut line)
        .await?;
    if read == 0 {
        return Ok(None);
    }
    if line.last() != Some(&b'\n') {
        // Sem quebra de linha: ou a mensagem passou do limite, ou a conexão
        // caiu no meio dela.
        return if line.len() > MAX_LINE {
            Err(CodecError::TooLong)
        } else {
            Ok(None)
        };
    }
    Ok(Some(serde_json::from_slice(&line)?))
}

/// Escreve uma mensagem e esvazia o buffer.
pub async fn write_message<T, W>(writer: &mut W, message: &T) -> Result<(), CodecError>
where
    T: Serialize,
    W: AsyncWrite + Unpin,
{
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientMessage, Command};
    use tokio::io::BufReader;

    #[tokio::test]
    async fn le_o_que_escreveu_e_para_no_fim() {
        let mut buf = Vec::new();
        let first = ClientMessage::Hello { protocol: 1 };
        let second = ClientMessage::Request {
            id: 1,
            command: Command::Status,
        };
        write_message(&mut buf, &first).await.unwrap();
        write_message(&mut buf, &second).await.unwrap();

        let mut reader = BufReader::new(buf.as_slice());
        let a: Option<ClientMessage> = read_message(&mut reader).await.unwrap();
        let b: Option<ClientMessage> = read_message(&mut reader).await.unwrap();
        let end: Option<ClientMessage> = read_message(&mut reader).await.unwrap();
        assert_eq!(a, Some(first));
        assert_eq!(b, Some(second));
        assert_eq!(end, None);
    }

    #[tokio::test]
    async fn recusa_linha_longa_demais() {
        let long = vec![b'a'; MAX_LINE + 10];
        let mut reader = BufReader::new(long.as_slice());
        let result: Result<Option<ClientMessage>, _> = read_message(&mut reader).await;
        assert!(matches!(result, Err(CodecError::TooLong)));
    }

    #[tokio::test]
    async fn recusa_json_invalido() {
        let mut reader = BufReader::new(&b"{\"type\":\"oi\"}\n"[..]);
        let result: Result<Option<ClientMessage>, _> = read_message(&mut reader).await;
        assert!(matches!(result, Err(CodecError::Invalid(_))));
    }
}
