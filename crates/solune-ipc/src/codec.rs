//! Framing: uma mensagem JSON por linha, terminada em `\n`.

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Tamanho máximo de uma linha. Uma lista de reprodução de milhares de
/// arquivos cabe com folga; o limite só impede que um par com defeito faça o
/// outro alocar sem fim.
pub const MAX_LINE: usize = 1024 * 1024;

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
///
/// `line` guarda o que já chegou da mensagem em andamento. Quem lê dentro de
/// um `select!` deve manter o mesmo `Vec` entre as chamadas: se a leitura for
/// cancelada no meio da linha, a próxima chamada continua de onde parou.
pub async fn read_message<T, R>(reader: &mut R, line: &mut Vec<u8>) -> Result<Option<T>, CodecError>
where
    T: DeserializeOwned,
    R: AsyncBufRead + Unpin,
{
    let budget = (MAX_LINE + 1).saturating_sub(line.len()) as u64;
    (&mut *reader)
        .take(budget)
        .read_until(b'\n', &mut *line)
        .await?;
    if line.last() != Some(&b'\n') {
        // Sem quebra de linha: ou a mensagem passou do limite, ou a conexão
        // caiu no meio dela.
        return if line.len() > MAX_LINE {
            Err(CodecError::TooLong)
        } else {
            Ok(None)
        };
    }
    let message = serde_json::from_slice(line);
    line.clear();
    Ok(Some(message?))
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
        let mut line = Vec::new();
        let a: Option<ClientMessage> = read_message(&mut reader, &mut line).await.unwrap();
        let b: Option<ClientMessage> = read_message(&mut reader, &mut line).await.unwrap();
        let end: Option<ClientMessage> = read_message(&mut reader, &mut line).await.unwrap();
        assert_eq!(a, Some(first));
        assert_eq!(b, Some(second));
        assert_eq!(end, None);
    }

    #[tokio::test]
    async fn recusa_linha_longa_demais() {
        let long = vec![b'a'; MAX_LINE + 10];
        let mut reader = BufReader::new(long.as_slice());
        let result: Result<Option<ClientMessage>, _> =
            read_message(&mut reader, &mut Vec::new()).await;
        assert!(matches!(result, Err(CodecError::TooLong)));
    }

    #[tokio::test]
    async fn recusa_json_invalido() {
        let mut reader = BufReader::new(&b"{\"type\":\"oi\"}\n"[..]);
        let result: Result<Option<ClientMessage>, _> =
            read_message(&mut reader, &mut Vec::new()).await;
        assert!(matches!(result, Err(CodecError::Invalid(_))));
    }

    #[tokio::test]
    async fn leitura_cancelada_no_meio_da_linha_continua_de_onde_parou() {
        use std::time::Duration;
        use tokio::io::AsyncWriteExt;

        let (mut tx, rx) = tokio::io::duplex(64);
        let mut reader = BufReader::new(rx);
        let mut line = Vec::new();
        let mut full = serde_json::to_vec(&ClientMessage::Request {
            id: 9,
            command: Command::Status,
        })
        .unwrap();
        full.push(b'\n');
        let (first, rest) = full.split_at(10);

        tx.write_all(first).await.unwrap();
        // Metade da linha chegou e a leitura é cancelada, como num select!
        // em que outro ramo ganha.
        let cancelled = tokio::time::timeout(
            Duration::from_millis(20),
            read_message::<ClientMessage, _>(&mut reader, &mut line),
        )
        .await;
        assert!(cancelled.is_err());

        tx.write_all(rest).await.unwrap();
        let message: Option<ClientMessage> = read_message(&mut reader, &mut line).await.unwrap();
        assert_eq!(
            message,
            Some(ClientMessage::Request {
                id: 9,
                command: Command::Status
            })
        );
        assert!(line.is_empty());
    }
}
