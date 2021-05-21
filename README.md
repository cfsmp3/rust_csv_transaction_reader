A very simple transaction processor in Rust.

Notes:

- Some of the specs are unclear. For example, a chargeback freezes the account, but that's just a flag - it doesn't have any effect (such as rejecting more transaction). This should be detailed.
- To avoid spending more time than allocated, tests are not super exhaustive, and there's some assumptions about the input file being sane. Definitely in a production system I wouldn't assume this to be true.
- The output explanation in the specs is missing the "locked" field for the non-tabbed example.
- Precision: I'm not doing anything with that. If the input matches specs (i.e. 4 or less decimal places) then the output will also match that, as you can't get more than 4 decimal places from 4 or less decimal places unless you are making division.
- Run with debug: RUST_LOG=debug cargo run -- test_files/a_bit_of_everything.csv
- The specs doesn't mention signs. I'm assuming they are not there and that the transaction type determines it. So withdrawing a negative amount of things are that is untested behavior.
- Using a hashtable to keep track of transactions. I'm assuming only the deposits can be disputed so the hashtable only contains that.
- The funds total is redundant in that it's always a sum, but I've keep it as a field anyway as it helped a bit with tests.
- Code is a bit on the unwrap() happy side due to some promised being made about the input.



