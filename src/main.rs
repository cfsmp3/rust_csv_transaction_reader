use csv::{Reader, StringRecord};
use log::debug;
use rust_decimal::prelude::*;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::env;
use std::error::Error;

type ClientId = u16; // client column is a valid u16 client ID
type TransactionId = u32; // the tx is a valid u32 transaction ID

#[derive(Debug)]
enum PaymentErrors {
    WrongArgumentCount,
    ImportCsv,
}

#[derive(Debug, PartialEq)]
enum TransactionStatus {
    OK,
    Disputed,
    Chargedback,
}

#[derive(Debug, PartialEq, Copy, Clone)]
enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

#[derive(Debug, PartialEq)]
struct Transaction {
    tx_type: TransactionType,
    client_id: ClientId,
    tx_id: TransactionId,
    amount: Option<Decimal>,
    status: TransactionStatus,
}

/*
columns type, client, tx, and amount. You can assume the type is a string, the
client column is a valid u16 client ID, the tx is a valid u32 transaction ID, and
the amount is a decimal value with a precision of up to four places past the decimal.
*/
impl TryFrom<StringRecord> for Transaction {
    type Error = &'static str;

    fn try_from(record: StringRecord) -> Result<Transaction, &'static str> {
        let tx_type = record.get(0).unwrap();
        let tx_type = match tx_type {
            "deposit" => TransactionType::Deposit,
            "withdrawal" => TransactionType::Withdrawal,
            "dispute" => TransactionType::Dispute,
            "resolve" => TransactionType::Resolve,
            "chargeback" => TransactionType::Chargeback,
            _ => return Err("Unknown transaction type"),
        };

        // All these unwraps are safe assuming that the input is sane; the problem
        // statement guarantees that.
        let client_id: ClientId = record.get(1).unwrap().trim().parse::<ClientId>().unwrap();
        let tx_id: TransactionId = record
            .get(2)
            .unwrap()
            .trim()
            .parse::<TransactionId>()
            .unwrap();
        let amount = match record.get(3) {
            None => None,
            Some ("") => None,
            Some (something) => Some(Decimal::from_str(something.trim()).unwrap()),
        };

        Ok(Transaction {
            tx_type,
            client_id,
            tx_id,
            amount,
            status: TransactionStatus::OK,
        })
    }
}

#[test]
fn test_record_to_transaction() {
    /* Deposits */
    let tx_deposit =
        Transaction::try_from(StringRecord::from(vec!["deposit", "1", "1", "1.0"])).unwrap();
    assert_eq!(
        tx_deposit,
        Transaction {
            tx_type: TransactionType::Deposit,
            client_id: 1,
            tx_id: 1,
            amount: Some(Decimal::from_str("1.0").unwrap()),
            status: TransactionStatus::OK
        }
    );

    /* Transaction inequality */
    assert_ne!(
        tx_deposit,
        Transaction {
            tx_type: TransactionType::Withdrawal,
            client_id: 1,
            tx_id: 1,
            amount: Some(Decimal::from_str("1.0").unwrap()),
            status: TransactionStatus::OK
        }
    );
}

struct PaymentEngine {
    accounts: HashMap<ClientId, Account>,
    transactions: HashMap<TransactionId, Transaction>, // We need to keep this to deal with disputes. In a non-toy implementation this doesn't belong in memory though
}

#[derive(Debug)]
struct Account {
    client_id: ClientId,
    num_transactions: u32,
    funds_available: Decimal,
    funds_held: Decimal,
    funds_total: Decimal, // TODO: Possibly redundant but let's keep around for now for basic sanity check
    locked: bool,
}

impl PaymentEngine {
    fn new() -> PaymentEngine {
        PaymentEngine {
            accounts: HashMap::new(),
            transactions: HashMap::new(),
        }
    }

    fn process_transaction(&mut self, transaction: Transaction) {
        let account_ref = self.accounts.get_mut(&transaction.client_id).unwrap();
        account_ref.num_transactions += 1;
        debug!(
            "client transactions now, num_transactions: {}",
            account_ref.num_transactions
        );
        debug!(
            "Processing transaction {:?}, {:?}",
            account_ref, transaction
        );
        match transaction.tx_type {
            TransactionType::Deposit => {
                assert!(!self.transactions.contains_key(&transaction.tx_id));
                let amount = transaction.amount.unwrap();
                account_ref.funds_available += amount;
                account_ref.funds_total += amount;
                debug!("Funds added!");
                // Assumption here: Only Deposits can be disputed so we don't store the rest
                self.transactions.insert(transaction.tx_id, transaction); // Adding it at the end avoid ownership BS
            }
            TransactionType::Withdrawal => {
                let amount = transaction.amount.unwrap();
                if account_ref.funds_available >= amount {
                    account_ref.funds_available -= amount;
                    account_ref.funds_total -= amount;
                    debug!("Funds withdrawn!");
                } else {
                    debug!(
                        "   (transaction declined, not enough funds ({} < {})!",
                        account_ref.funds_available, amount
                    );
                }
            }
            TransactionType::Dispute => {
                let maybe_orig_txt = self.transactions.get_mut(&transaction.tx_id);
                if let Some(mut orig_txt) = maybe_orig_txt {
                    debug!("Found disputed transaction {:?}", orig_txt);
                    if orig_txt.status == TransactionStatus::OK
                        && orig_txt.client_id == transaction.client_id
                    {
                        debug!(" OK, it can be disputed.");
                        orig_txt.status = TransactionStatus::Disputed;
                        let amount = orig_txt.amount.unwrap();
                        account_ref.funds_available -= amount;
                        account_ref.funds_held += amount;
                    }
                }
            }
            TransactionType::Resolve => {
                let maybe_orig_txt = self.transactions.get_mut(&transaction.tx_id);
                if let Some(mut orig_txt) = maybe_orig_txt {
                    debug!("Found disputed transaction {:?}", orig_txt);
                    if orig_txt.status == TransactionStatus::Disputed
                        && orig_txt.client_id == transaction.client_id
                    {
                        debug!(" OK, it can be resolved.");
                        orig_txt.status = TransactionStatus::OK;
                        let amount = orig_txt.amount.unwrap();
                        account_ref.funds_available += amount;
                        account_ref.funds_held -= amount;
                    }
                }
            }
            TransactionType::Chargeback => {
                let maybe_orig_txt = self.transactions.get_mut(&transaction.tx_id);
                if let Some(mut orig_txt) = maybe_orig_txt {
                    debug!("Found disputed transaction {:?}", orig_txt);
                    if orig_txt.status == TransactionStatus::Disputed
                        && orig_txt.client_id == transaction.client_id
                    {
                        debug!(" OK, it can be chargedback.");
                        orig_txt.status = TransactionStatus::Chargedback;
                        let amount = orig_txt.amount.unwrap();
                        account_ref.funds_available += amount;
                        account_ref.funds_held -= amount;
                        account_ref.locked = true; // If a chargeback occurs the client's account should be immediately frozen.
                    }
                }
            }
        };
        debug!("Account status after this transaction: {:?}", account_ref);
    }

    fn import_csv(&mut self, filename: &str) -> Result<(), Box<dyn Error>> {
        let mut rdr = Reader::from_path(filename)?;
        for result in rdr.records() {
            let record = result?;
            debug!("{:?}", record);
            assert!(record.len() ==  3 || record.len() == 4); // It should always be 4 but since amount is optional maybe the comma also is

            let transaction = Transaction::try_from(record).unwrap(); // Assuming non-fail since input is guaranteed to be sane
            debug!("Transaction: {:?}", transaction);

            if !self.accounts.contains_key(&transaction.client_id) {
                let account = Account {
                    client_id: transaction.client_id,
                    num_transactions: 0,
                    funds_available: Decimal::new(0, 0),
                    funds_held: Decimal::new(0, 0),
                    funds_total: Decimal::new(0, 0),
                    locked: false,
                };
                self.accounts.insert(transaction.client_id, account);
                debug!("Account created for new client");
            }
            self.process_transaction(transaction);
        }
        Ok(())
    }

    fn export_accounts(&self) {
        println!("client,available,held,total,locked");
        for client_id in self.accounts.keys() {
            let account_ref = self.accounts.get(client_id).unwrap();
            println!(
                "{},{},{},{},{}",
                client_id,
                account_ref.funds_available,
                account_ref.funds_held,
                account_ref.funds_total,
                account_ref.locked
            );
        }
    }
}

fn main() -> Result<(), PaymentErrors> {
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        return Err(PaymentErrors::WrongArgumentCount);
    }
    let mut engine = PaymentEngine::new();
    engine
        .import_csv(args[1].as_str())
        .map_err(|_| -> PaymentErrors { PaymentErrors::ImportCsv })?;
    engine.export_accounts();
    Ok(())
}
