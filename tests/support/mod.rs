use std::ops::Deref;

use prelay_server::storage::Storage;

/// 集成测试的存储句柄：包装库内测试基座，附带 `Deref` 方便直接调用存储方法。
pub struct TestStorage(Storage);

impl TestStorage {
    pub fn storage(&self) -> &Storage {
        &self.0
    }
}

impl Deref for TestStorage {
    type Target = Storage;

    fn deref(&self) -> &Self::Target {
        self.storage()
    }
}

pub async fn test_storage() -> TestStorage {
    TestStorage(prelay_server::test_support::test_storage().await)
}
