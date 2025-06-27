use super::client::TONRpcClient;
use relayer_base::database::Database;

pub struct TONSubscriber<DB: Database> {
    _client: TONRpcClient,
    _db: DB,
}
