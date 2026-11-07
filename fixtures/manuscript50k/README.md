# 50,000-word test manuscript

`pride-and-prejudice-50k.txt`: the first 27 chapters (51,867 words) of *Pride and Prejudice* by Jane Austen (1813), from Project Gutenberg ebook #1342. Public domain. Chapters are separated by a line containing `=====`; each starts with its `CHAPTER` heading.

Used by the opt-in live test `live_ask_my_book_on_a_50k_word_manuscript` (SPEC §17 check 10, M4 acceptance): `MARGIN_LIVE_MODEL=1 cargo test --manifest-path src-tauri/Cargo.toml --lib live_ -- --nocapture`.
