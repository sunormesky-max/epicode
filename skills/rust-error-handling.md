# Rust Error Handling

## 描述
在 Rust 项目中实现健壮的错误处理模式，包括自定义错误类型、错误转换和统一的错误响应格式。

## 触发条件
- 需要设计新的错误类型
- 处理多个 crate 的错误转换
- 构建 REST API 的错误响应

## 步骤

### 1. 定义自定义错误类型

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("数据库错误: {0}")]
    Database(#[from] sqlx::Error),
    
    #[error("验证失败: {0}")]
    Validation(String),
    
    #[error("未找到: {0}")]
    NotFound(String),
    
    #[error("内部错误")]
    Internal,
}

pub type Result<T> = std::result::Result<T, AppError>;
```

### 2. 实现错误转换

```rust
impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Internal
    }
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            AppError::Database(_) => (500, "数据库错误"),
            AppError::Validation(msg) => (400, &msg),
            AppError::NotFound(msg) => (404, &msg),
            AppError::Internal => (500, "内部错误"),
        };
        
        (status, Json(json!({"error": message }))).into_response()
    }
}
```

### 3. 使用 ? 运算符

```rust
pub async fn get_user(id: i64, pool: &PgPool) -> Result<User> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound(format!("用户 {} 不存在", id)))?;
    
    Ok(user)
}
```

## 示例

### 完整错误处理流程

```rust
use axum::{
    extract::{Path, State},
    Json,
};

async fn handler(
    Path(id): Path<i64>,
    State(pool): State<PgPool>,
) -> Result<Json<User>> {
    let user = get_user(id, &pool).await?;
    Ok(Json(user))
}
```

## 标签

- category: rust
- difficulty: intermediate
- language: rust
- author: 冬儿
- created: 2026-06-21

## 参考

- [Rust Error Handling](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
- [thiserror crate](https://docs.rs/thiserror/)
- [anyhow crate](https://docs.rs/anyhow/)
