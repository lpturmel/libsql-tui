use anyhow::Result;
use libsql::{Connection, Rows, Value};
use std::{fmt::Display, ops::Deref};

#[derive(Debug, Clone)]
pub struct LibSqlClient(pub Connection);

impl LibSqlClient {
    pub async fn query_owned(&self, sql: &str) -> Result<Table> {
        let mut rows: Rows = self.query(sql, ()).await?;

        let col_cnt = rows.column_count();
        let mut cols = Vec::with_capacity(col_cnt as usize);
        for i in 0..col_cnt {
            cols.push(rows.column_name(i).unwrap_or("").to_owned());
        }

        let mut out_rows = Vec::new();
        while let Some(row) = rows.next().await? {
            let mut vals = Vec::with_capacity(col_cnt as usize);
            for i in 0..col_cnt {
                vals.push(ValueWrapper(row.get_value(i)?));
            }
            out_rows.push(vals);
        }

        Ok(Table {
            columns: cols,
            rows: out_rows,
        })
    }
}

#[derive(Debug)]
pub struct ValueWrapper(Value);

impl Display for ValueWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = &self.0;
        match inner {
            Value::Null => write!(f, "NULL"),
            Value::Integer(i) => write!(f, "{i}"),
            Value::Real(x) => write!(f, "{x}"),
            Value::Text(s) => write!(f, "{s}"),
            Value::Blob(bytes) => {
                let shown = bytes
                    .iter()
                    .take(16)
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                if bytes.len() > 16 {
                    write!(f, "{shown} … ({} bytes)", bytes.len())
                } else {
                    write!(f, "{shown}")
                }
            }
        }
    }
}

impl Deref for LibSqlClient {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<ValueWrapper>>,
}
