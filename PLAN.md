# Project Plan: PgQuack
### Direct Query Engine for PostgreSQL Dumps (Zero-Restore, DuckDB-powered)

*Trạng thái: Draft / Proposal*
*Ngày cập nhật: 2026-07-20*

**Nguyên tắc thiết kế cốt lõi:** *"Đúng và không crash" quan trọng hơn "nhanh"* ở giai đoạn đầu. Một parser sai lệch dữ liệu âm thầm nguy hiểm hơn nhiều so với một parser chậm.

---

## 1. Tổng quan dự án (Executive Summary)

**PgQuack** là một CLI tool (và về sau là DuckDB Extension) cho phép thực thi SQL kiểu OLAP trực tiếp trên file dump PostgreSQL (`.sql`, `.sql.gz`, `.sql.zst`) mà **không cần restore** vào một Postgres instance đang chạy. Công cụ tận dụng DuckDB làm execution engine và Apache Arrow làm định dạng trung gian, biến file backup thành nguồn dữ liệu có thể truy vấn gần như tức thì cho các tác vụ debug, forensics, và ETL/migration.

---

## 2. Vấn đề & Giải pháp

### 2.1. Nỗi đau hiện tại
- **Thời gian chết:** Query 1 dòng trong file dump 50GB đòi hỏi `pg_restore` (hàng giờ), tốn RAM, tốn gấp đôi dung lượng đĩa.
- **Debug/Forensics cồng kềnh:** Kiểm tra dữ liệu lịch sử từ backup cũ cần dựng lại cả một DB server.
- **Lãng phí trong ETL/Migration:** Phải restore về Postgres rồi mới extract ra Parquet/Clickhouse — một bước trung gian thừa I/O.

### 2.2. Giải pháp PgQuack
- **Zero-Restore:** Query trực tiếp `.sql` / `.sql.gz` / `.sql.zst`.
- **Local-first:** Không cần Docker hay Postgres server.
- **OLAP tốc độ cao (sau lần query đầu):** Nhờ cache Parquet tự động — xem mục 5.3.
- **An toàn dữ liệu:** Không bao giờ trả kết quả sai âm thầm; lỗi parse phải được log rõ ràng, không được nuốt (swallow) silently.

---

## 3. Kiến trúc kỹ thuật

```text
[ File Dump (.sql / .sql.gz / .sql.zst) ]
              │
              ▼
[ Layer 0: Cache Lookup ] ── Đã có bản Parquet cache? ──► [ Layer 4: Query Engine ]
              │ (chưa có / cache miss)
              ▼
[ Layer 1: Stream Decompressor ] (flate2 / zstd, streaming)
              │
              ▼
[ Layer 2: Metadata Parser ] ──► CREATE TABLE ──► Schema / Data Dictionary
              │
              ▼
[ Layer 3: Data Stream Parser ] ──► COPY ... FROM stdin ──► Apache Arrow (chunked, batch 100k rows)
              │                         │
              │                 (dòng lỗi/malformed → log + skip, không crash)
              ▼
[ Layer 3.5: Cache Writer ] ──► Ghi Arrow chunk xuống Parquet cache (background, async)
              │
              ▼
[ Layer 4: Query Engine ] ──► DuckDB (embed qua C-API) ──► Execute SQL & Return Results
```

Layer 0 (Cache Lookup) và Layer 3.5 (Cache Writer) được đưa vào kiến trúc **ngay từ đầu thiết kế**. Lần query đầu tiên trên một file dump sẽ chậm (full scan + build cache), nhưng mọi query sau đó trên cùng file sẽ nhanh như đọc Parquet thuần.

### 3.1. Các thành phần cốt lõi
1. **Schema Extractor** — parse `CREATE TABLE`, kiểu dữ liệu, constraints, khóa chính.
2. **COPY Parser** — xử lý delimiter, escape (`\t`, `\n`, `\N`, `\xHH`), an toàn với dữ liệu malformed.
3. **Type Mapper** — map kiểu Postgres → Arrow/DuckDB (`JSONB`→`JSON`, `Array`→`List`, `UUID`→`UUID/String`, `Numeric`→`Decimal`).
4. **Cache Manager** — quản lý vòng đời file `.parquet` cache: tạo, invalidate (theo hash + mtime của file dump gốc), dọn dẹp.
5. **Error Reporter** — thu thập và báo cáo rõ ràng các dòng bị skip (số dòng, lý do, mẫu dữ liệu lỗi) sau mỗi lần chạy.
6. **DuckDB Integration** — CLI wrapper trước (Phase 1-2), Extension sau (Phase 3).

---

## 4. Thách thức kỹ thuật & Chiến lược giảm thiểu

| Thách thức | Mức độ | Chiến lược giải quyết |
|---|---|---|
| File nén (gzip/zstd) | Cao | Sequential scan cho lần đầu; **cache Parquet ngay sau đó** để các lần sau không phải giải nén lại |
| Escape characters | Trung bình-Cao | Test corpus với ≥20 file dump thực tế đa dạng (nhiều version Postgres, nhiều charset) ngay từ Phase 1; fuzz testing cho parser |
| Custom Dump (`-Fc`) | Cao | Bỏ qua ở các phiên bản đầu, xem lại sau nếu có nhu cầu rõ ràng từ người dùng |
| Kiểu dữ liệu phức tạp | Trung bình | Map cơ bản trước; JSONB/Array ở Phase 2; PostGIS/Citus ngoài phạm vi ban đầu |
| Tràn RAM | Thấp | Streaming & batch (100k dòng/chunk), không load toàn file vào RAM |
| **Dữ liệu sai lệch âm thầm** | **Cao** | Không bao giờ "đoán" khi gặp dữ liệu không parse được — log + skip + báo cáo rõ ràng cho người dùng, kèm exit code khác 0 nếu có dòng bị skip |
| **Độ khó viết DuckDB Extension** | Trung bình | Trì hoãn việc viết Extension sang Phase 3; Phase 1-2 chỉ cần CLI gọi DuckDB qua C-API ở chế độ embedded, không cần custom table function phức tạp |

---

## 5. Công nghệ đề xuất (Tech Stack)

- **Ngôn ngữ:** Rust (ưu tiên — memory safety cho parser xử lý dữ liệu không tin cậy, ecosystem crates tốt cho streaming/compression).
- **Core Engine:** DuckDB, tích hợp qua `duckdb-rs` (Rust binding cho C-API) ở giai đoạn đầu — **không** viết Extension ngay.
- **In-memory Format:** Apache Arrow (`arrow-rs`).
- **Compression:** `flate2` (gzip), `zstd` crate.
- **Cache format:** Parquet (`parquet` crate) — dùng làm lớp cache tăng tốc, không phải deliverable chính.
- **Testing:** `proptest`/`arbitrary` cho fuzz testing parser; corpus test files lưu trong repo (hoặc submodule riêng do dung lượng).
- **Build:** `cargo`.

---

## 6. Lộ trình phát triển (Roadmap)

### Phase 0: Nền tảng & Test Corpus (2-3 tuần)
- Thu thập/tạo ≥20 file dump mẫu: nhiều version Postgres (12-17), nhiều kiểu dữ liệu, có cả trường hợp escape phức tạp, Unicode, NULL, chuỗi rỗng.
- Viết harness test tự động: parse → so sánh với kết quả `pg_restore` thật trên cùng dữ liệu.
- Định nghĩa format báo cáo lỗi (Error Reporter) ngay từ đầu để toàn bộ code sau này tuân theo.

### Phase 1: MVP lõi (Tháng 1-2, +1 tuần buffer)
- CLI tool cơ bản, đọc file `.sql` text (Plain format) **không nén**.
- Chỉ parse `CREATE TABLE` và `COPY ... FROM stdin`; bỏ qua `INSERT` để tối ưu tốc độ triển khai.
- Map kiểu dữ liệu cơ bản: `INT`, `BIGINT`, `TEXT`, `VARCHAR`, `BOOLEAN`, `TIMESTAMP`.
- Tích hợp DuckDB qua `duckdb-rs`, hỗ trợ `SELECT`, `COUNT`, `WHERE`.
- Error Reporter hoạt động: mọi dòng lỗi phải được log, không được crash chương trình.
- **Milestone:** `pgquack query dump.sql "SELECT count(*) FROM users"` chạy đúng trên toàn bộ test corpus Phase 0, không có dòng nào bị sai lệch âm thầm.

### Phase 2: Nén + Cache/Index (Tháng 3)
- Hỗ trợ đọc `.gz` và `.zst` (streaming decompression).
- **Cache Manager:** lần query đầu → build Parquet cache trong background; các lần sau đọc thẳng cache (kiểm tra qua hash + mtime file gốc để tự invalidate).
- Hoàn thiện COPY Parser: escape characters, NULL (`\N`), ký tự đặc biệt.
- Mở rộng Type Mapper: `JSON/JSONB`, `UUID`, `NUMERIC`, `Array` 1 chiều.
- **Milestone:** Query lần 2 trở đi trên cùng file dump nhanh tương đương đọc Parquet thuần; chạy ổn định trên dump thực tế từ production.

### Phase 3: DuckDB Extension (Tháng 4-5)
- Đóng gói CLI logic thành DuckDB Extension (`INSTALL pgquack; LOAD pgquack;`).
- Cho phép query trực tiếp trong DuckDB CLI, DBeaver, Jupyter Notebook.
- Tận dụng lại toàn bộ Cache Manager đã ổn định từ Phase 2 — không phát sinh rủi ro mới.
- **Milestone:** Release bản Beta, công bố lên DuckDB Extension Registry (hoặc GitHub Release nếu registry chưa nhận).

### Phase 4: Advanced Features (Tháng 6+)
- Parallel parsing (chia nhỏ file dump để parse đa luồng — cẩn trọng vì text khó chia nhỏ đúng ranh giới dòng).
- Hỗ trợ Custom Directory dump (`pg_dump -Fd`).
- CLI interactive với syntax highlight.
- (Có nhu cầu rõ) đánh giá lại việc hỗ trợ Custom Binary Format `-Fc`.

---

## 7. Phạm vi dự án (Scope & Out of Scope)

### Trong phạm vi (In-Scope)
- Đọc và query file dump Plain Text (`-Fp`), nén hoặc không nén (gzip/zstd).
- SQL: `SELECT`, `WHERE`, `GROUP BY`, `JOIN` giữa các bảng trong cùng dump.
- CLI tool (Phase 1-2), DuckDB Extension (Phase 3+).
- Cache/index tự động để tăng tốc query lặp lại.

### Ngoài phạm vi (Out-of-Scope)
- Không thay thế `pg_restore` để phục hồi database thật.
- Không hỗ trợ Custom Binary Format (`-Fc`) ở các phiên bản đầu.
- Không hỗ trợ ghi/UPDATE/DELETE vào file dump (read-only tuyệt đối).
- Không hỗ trợ extension đặc thù của Postgres (PostGIS phức tạp, Citus) ở phiên bản đầu.

---

## 8. Chỉ số đo lường thành công (Success Metrics)

**Nhóm metric kỹ thuật (ưu tiên, đo được ngay trong quá trình phát triển):**
1. **Độ chính xác:** 100% khớp dữ liệu so với `pg_restore` thật trên toàn bộ test corpus — đo tự động sau mỗi commit (CI).
2. **Coverage định dạng:** số lượng/tỷ lệ loại dump (theo version Postgres, loại dữ liệu) được test corpus bao phủ.
3. **Tốc độ parse lần đầu (cold):** stream-parse 10GB dump < 2 phút trên máy tiêu chuẩn (M1 Mac / i7).
4. **Tốc độ query sau cache (warm):** < 1/10 thời gian cold query nhờ Parquet cache.
5. **RAM sử dụng:** không vượt quá 2GB khi xử lý file dump 50GB.
6. **Tỷ lệ dòng bị skip do lỗi parse:** phải bằng 0 trên test corpus chuẩn; nếu >0 trên dump thực tế của người dùng, phải được báo cáo rõ ràng (không âm thầm).

**Nhóm metric adoption (thứ yếu, đo sau khi có bản Beta):**
7. Số lượng dump thực tế (từ cộng đồng) đã test thành công.
8. Được tích hợp vào ít nhất 1 pipeline ETL/CI-CD open-source.
9. GitHub stars — chỉ theo dõi tham khảo, không đặt làm mục tiêu chính.

---

## 9. Rủi ro tổng thể & kế hoạch dự phòng

| Rủi ro | Tác động | Kế hoạch dự phòng |
|---|---|---|
| Parser không xử lý hết edge case escape trong thời gian dự kiến | Trễ Phase 1 | Buffer 1 tuần đã tính sẵn; nếu vẫn trễ, cắt bớt phạm vi type mapping (chỉ giữ INT/TEXT/BOOL) để giữ đúng milestone lõi |
| Viết DuckDB Extension khó hơn dự kiến | Trễ Phase 3 | CLI tool (Phase 1-2) đã là sản phẩm dùng được độc lập — có thể dừng ở đó và release như một CLI tool thay vì Extension |
| Hiệu năng cache Parquet không đạt kỳ vọng | Giảm giá trị "warm query" | Vẫn có giá trị so với `pg_restore`; công bố rõ số liệu thực đo thay vì con số kỳ vọng |
| Không thu hút được cộng đồng đóng góp test corpus | Coverage thấp | Tự tạo synthetic dump đa dạng bằng script, không phụ thuộc hoàn toàn vào cộng đồng |

---

*Người lập kế hoạch: [Tên của bạn]*
*Phiên bản: Draft*
*Trạng thái: Draft / Proposal*
