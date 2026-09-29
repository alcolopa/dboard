import Foundation

public final class MockDatabaseGenerator {
    public static func makePostgresSample() -> (DatabaseMetadata, [String: [DataRow]]) {
        // Tables
        let userCols: [ColumnDefinition] = [
            ColumnDefinition(name: "id", ordinalPosition: 1, dataTypeName: "integer", isPrimaryKey: true, isNullable: false, defaultValue: "nextval('users_id_seq'::regclass)"),
            ColumnDefinition(name: "email", ordinalPosition: 2, dataTypeName: "varchar(255)", isNullable: false),
            ColumnDefinition(name: "name", ordinalPosition: 3, dataTypeName: "varchar(100)", isNullable: false),
            ColumnDefinition(name: "role", ordinalPosition: 4, dataTypeName: "varchar(50)", isNullable: false, defaultValue: "'member'"),
            ColumnDefinition(name: "status", ordinalPosition: 5, dataTypeName: "varchar(30)", isNullable: false, defaultValue: "'active'"),
            ColumnDefinition(name: "is_verified", ordinalPosition: 6, dataTypeName: "boolean", isNullable: false, defaultValue: "true"),
            ColumnDefinition(name: "balance", ordinalPosition: 7, dataTypeName: "numeric(10,2)", isNullable: false, defaultValue: "0.00"),
            ColumnDefinition(name: "profile_data", ordinalPosition: 8, dataTypeName: "jsonb", isNullable: true),
            ColumnDefinition(name: "created_at", ordinalPosition: 9, dataTypeName: "timestamp with time zone", isNullable: false, defaultValue: "CURRENT_TIMESTAMP")
        ]

        let usersTable = TableMetadata(
            schemaName: "public",
            name: "users",
            type: .table,
            estimatedRows: 5,
            sizeBytes: 65536,
            comment: "Application core users table",
            columns: userCols,
            primaryKeyColumnNames: ["id"]
        )

        let productCols: [ColumnDefinition] = [
            ColumnDefinition(name: "id", ordinalPosition: 1, dataTypeName: "integer", isPrimaryKey: true, isNullable: false, defaultValue: "nextval('products_id_seq'::regclass)"),
            ColumnDefinition(name: "sku", ordinalPosition: 2, dataTypeName: "varchar(50)", isNullable: false),
            ColumnDefinition(name: "title", ordinalPosition: 3, dataTypeName: "varchar(200)", isNullable: false),
            ColumnDefinition(name: "price", ordinalPosition: 4, dataTypeName: "numeric(10,2)", isNullable: false),
            ColumnDefinition(name: "stock_quantity", ordinalPosition: 5, dataTypeName: "integer", isNullable: false, defaultValue: "0"),
            ColumnDefinition(name: "is_active", ordinalPosition: 6, dataTypeName: "boolean", isNullable: false, defaultValue: "true"),
            ColumnDefinition(name: "tags", ordinalPosition: 7, dataTypeName: "text[]", isNullable: true),
            ColumnDefinition(name: "created_at", ordinalPosition: 8, dataTypeName: "timestamp with time zone", isNullable: false, defaultValue: "CURRENT_TIMESTAMP")
        ]

        let productsTable = TableMetadata(
            schemaName: "public",
            name: "products",
            type: .table,
            estimatedRows: 4,
            sizeBytes: 49152,
            comment: "Catalog items and inventory",
            columns: productCols,
            primaryKeyColumnNames: ["id"]
        )

        let orderCols: [ColumnDefinition] = [
            ColumnDefinition(name: "id", ordinalPosition: 1, dataTypeName: "integer", isPrimaryKey: true, isNullable: false, defaultValue: "nextval('orders_id_seq'::regclass)"),
            ColumnDefinition(name: "user_id", ordinalPosition: 2, dataTypeName: "integer", isForeignKey: true, isNullable: false, foreignTable: "users", foreignColumn: "id"),
            ColumnDefinition(name: "order_number", ordinalPosition: 3, dataTypeName: "varchar(50)", isNullable: false),
            ColumnDefinition(name: "total_amount", ordinalPosition: 4, dataTypeName: "numeric(10,2)", isNullable: false),
            ColumnDefinition(name: "status", ordinalPosition: 5, dataTypeName: "varchar(30)", isNullable: false, defaultValue: "'processing'"),
            ColumnDefinition(name: "shipping_info", ordinalPosition: 6, dataTypeName: "jsonb", isNullable: true),
            ColumnDefinition(name: "placed_at", ordinalPosition: 7, dataTypeName: "timestamp with time zone", isNullable: false, defaultValue: "CURRENT_TIMESTAMP")
        ]

        let ordersTable = TableMetadata(
            schemaName: "public",
            name: "orders",
            type: .table,
            estimatedRows: 4,
            sizeBytes: 32768,
            comment: "Customer orders",
            columns: orderCols,
            primaryKeyColumnNames: ["id"]
        )

        let activeCustView = TableMetadata(
            schemaName: "public",
            name: "v_active_customers",
            type: .view,
            estimatedRows: 4,
            columns: userCols,
            primaryKeyColumnNames: []
        )

        let orderSummaryMatView = TableMetadata(
            schemaName: "public",
            name: "mv_order_summaries",
            type: .materializedView,
            estimatedRows: 4,
            columns: orderCols,
            primaryKeyColumnNames: []
        )

        let auditCols: [ColumnDefinition] = [
            ColumnDefinition(name: "id", ordinalPosition: 1, dataTypeName: "bigint", isPrimaryKey: true, isNullable: false),
            ColumnDefinition(name: "event_uuid", ordinalPosition: 2, dataTypeName: "uuid", isNullable: false),
            ColumnDefinition(name: "user_id", ordinalPosition: 3, dataTypeName: "integer", isForeignKey: true, isNullable: false, foreignTable: "users", foreignColumn: "id"),
            ColumnDefinition(name: "action", ordinalPosition: 4, dataTypeName: "varchar(80)", isNullable: false),
            ColumnDefinition(name: "status_code", ordinalPosition: 5, dataTypeName: "integer", isNullable: false, defaultValue: "200"),
            ColumnDefinition(name: "ip_address", ordinalPosition: 6, dataTypeName: "inet", isNullable: false),
            ColumnDefinition(name: "duration_ms", ordinalPosition: 7, dataTypeName: "numeric(8,2)", isNullable: false),
            ColumnDefinition(name: "payload", ordinalPosition: 8, dataTypeName: "jsonb", isNullable: true),
            ColumnDefinition(name: "created_at", ordinalPosition: 9, dataTypeName: "timestamp with time zone", isNullable: false, defaultValue: "CURRENT_TIMESTAMP")
        ]

        let audit10mTable = TableMetadata(
            schemaName: "public",
            name: "audit_events_10m",
            type: .table,
            estimatedRows: 10_000_000,
            sizeBytes: 2_147_483_648, // 2 GB
            comment: "High-scale partitioned telemetry & audit table with 10,000,000 rows",
            columns: auditCols,
            primaryKeyColumnNames: ["id"]
        )

        // Indexes
        let indexes = [
            IndexMetadata(name: "users_pkey", schemaName: "public", tableName: "users", isUnique: true, isPrimary: true, method: "BTREE", columnNames: ["id"], definition: "CREATE UNIQUE INDEX users_pkey ON public.users USING btree (id)"),
            IndexMetadata(name: "idx_users_email", schemaName: "public", tableName: "users", isUnique: true, isPrimary: false, method: "BTREE", columnNames: ["email"], definition: "CREATE UNIQUE INDEX idx_users_email ON public.users USING btree (email)"),
            IndexMetadata(name: "products_pkey", schemaName: "public", tableName: "products", isUnique: true, isPrimary: true, method: "BTREE", columnNames: ["id"], definition: "CREATE UNIQUE INDEX products_pkey ON public.products USING btree (id)"),
            IndexMetadata(name: "orders_pkey", schemaName: "public", tableName: "orders", isUnique: true, isPrimary: true, method: "BTREE", columnNames: ["id"], definition: "CREATE UNIQUE INDEX orders_pkey ON public.orders USING btree (id)"),
            IndexMetadata(name: "idx_orders_user_id", schemaName: "public", tableName: "orders", isUnique: false, isPrimary: false, method: "BTREE", columnNames: ["user_id"], definition: "CREATE INDEX idx_orders_user_id ON public.orders USING btree (user_id)"),
            IndexMetadata(name: "audit_10m_pkey", schemaName: "public", tableName: "audit_events_10m", isUnique: true, isPrimary: true, method: "BTREE", columnNames: ["id"], definition: "CREATE UNIQUE INDEX audit_10m_pkey ON public.audit_events_10m USING btree (id)"),
            IndexMetadata(name: "idx_audit_created_at", schemaName: "public", tableName: "audit_events_10m", isUnique: false, isPrimary: false, method: "BRIN", columnNames: ["created_at"], definition: "CREATE INDEX idx_audit_created_at ON public.audit_events_10m USING brin (created_at)")
        ]

        // Constraints
        let constraints = [
            ConstraintMetadata(name: "users_pkey", schemaName: "public", tableName: "users", type: .primaryKey, definition: "PRIMARY KEY (id)"),
            ConstraintMetadata(name: "users_email_key", schemaName: "public", tableName: "users", type: .unique, definition: "UNIQUE (email)"),
            ConstraintMetadata(name: "orders_user_id_fkey", schemaName: "public", tableName: "orders", type: .foreignKey, definition: "FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE", foreignTable: "users", foreignColumns: ["user_id"], referencedColumns: ["id"]),
            ConstraintMetadata(name: "audit_10m_pkey", schemaName: "public", tableName: "audit_events_10m", type: .primaryKey, definition: "PRIMARY KEY (id)")
        ]

        // Triggers
        let triggers = [
            TriggerMetadata(name: "trg_update_timestamp", schemaName: "public", tableName: "users", timing: "BEFORE", event: "UPDATE", functionName: "fn_update_timestamp", definition: "CREATE TRIGGER trg_update_timestamp BEFORE UPDATE ON users FOR EACH ROW EXECUTE FUNCTION fn_update_timestamp()"),
            TriggerMetadata(name: "trg_audit_orders", schemaName: "public", tableName: "orders", timing: "AFTER", event: "INSERT OR UPDATE", functionName: "fn_audit_orders", definition: "CREATE TRIGGER trg_audit_orders AFTER INSERT OR UPDATE ON orders FOR EACH ROW EXECUTE FUNCTION fn_audit_orders()")
        ]

        // Stored Procedures and Functions
        let routines = [
            RoutineMetadata(
                schemaName: "public",
                name: "process_monthly_invoices",
                routineType: .procedure,
                returnType: "void",
                arguments: "billing_cycle_id integer, dry_run boolean DEFAULT false",
                language: "plpgsql",
                definition: """
                CREATE OR REPLACE PROCEDURE process_monthly_invoices(billing_cycle_id integer, dry_run boolean DEFAULT false)
                LANGUAGE plpgsql
                AS $$
                DECLARE
                    v_customer RECORD;
                BEGIN
                    RAISE NOTICE 'Starting batch billing for cycle % (dry_run=%)', billing_cycle_id, dry_run;
                    FOR v_customer IN SELECT id, balance FROM users WHERE balance > 0 LOOP
                        IF NOT dry_run THEN
                            INSERT INTO orders (user_id, order_number, total_amount, status)
                            VALUES (v_customer.id, 'INV-' || to_char(NOW(), 'YYYYMM') || '-' || v_customer.id, v_customer.balance, 'processing');
                            UPDATE users SET balance = 0.00 WHERE id = v_customer.id;
                        END IF;
                    END LOOP;
                    COMMIT;
                    RAISE NOTICE 'Completed billing cycle successfully.';
                END;
                $$;
                """
            ),
            RoutineMetadata(
                schemaName: "public",
                name: "archive_audit_logs",
                routineType: .procedure,
                returnType: "void",
                arguments: "retention_days integer DEFAULT 90, batch_size integer DEFAULT 10000",
                language: "plpgsql",
                definition: """
                CREATE OR REPLACE PROCEDURE archive_audit_logs(retention_days integer DEFAULT 90, batch_size integer DEFAULT 10000)
                LANGUAGE plpgsql
                AS $$
                DECLARE
                    v_deleted_count integer := 0;
                    v_cutoff timestamp with time zone := NOW() - (retention_days || ' days')::interval;
                BEGIN
                    LOOP
                        DELETE FROM audit_events_10m
                        WHERE id IN (
                            SELECT id FROM audit_events_10m
                            WHERE created_at < v_cutoff
                            LIMIT batch_size
                        );
                        GET DIAGNOSTICS v_deleted_count = ROW_COUNT;
                        COMMIT;
                        EXIT WHEN v_deleted_count = 0;
                    END LOOP;
                END;
                $$;
                """
            ),
            RoutineMetadata(
                schemaName: "public",
                name: "reindex_all_tables",
                routineType: .procedure,
                returnType: "void",
                arguments: "target_schema varchar DEFAULT 'public'",
                language: "plpgsql",
                definition: """
                CREATE OR REPLACE PROCEDURE reindex_all_tables(target_schema varchar DEFAULT 'public')
                LANGUAGE plpgsql
                AS $$
                BEGIN
                    EXECUTE format('REINDEX SCHEMA %I;', target_schema);
                    RAISE NOTICE 'Reindexing completed for schema %', target_schema;
                END;
                $$;
                """
            ),
            RoutineMetadata(
                schemaName: "public",
                name: "calculate_tax",
                routineType: .function,
                returnType: "numeric(10,2)",
                arguments: "subtotal numeric, tax_rate numeric",
                language: "plpgsql",
                definition: """
                CREATE OR REPLACE FUNCTION calculate_tax(subtotal numeric, tax_rate numeric)
                RETURNS numeric
                LANGUAGE plpgsql
                IMMUTABLE
                AS $$
                BEGIN
                    RETURN ROUND(subtotal * (tax_rate / 100.0), 2);
                END;
                $$;
                """
            ),
            RoutineMetadata(
                schemaName: "public",
                name: "get_customer_lifetime_value",
                routineType: .function,
                returnType: "numeric(12,2)",
                arguments: "target_user_id integer",
                language: "plpgsql",
                definition: """
                CREATE OR REPLACE FUNCTION get_customer_lifetime_value(target_user_id integer)
                RETURNS numeric
                LANGUAGE plpgsql
                STABLE
                AS $$
                DECLARE
                    v_total numeric;
                BEGIN
                    SELECT coalesce(SUM(total_amount), 0.00) INTO v_total
                    FROM orders
                    WHERE user_id = target_user_id AND status != 'cancelled';
                    RETURN v_total;
                END;
                $$;
                """
            ),
            RoutineMetadata(
                schemaName: "public",
                name: "fn_update_timestamp",
                routineType: .function,
                returnType: "trigger",
                arguments: "",
                language: "plpgsql",
                definition: """
                CREATE OR REPLACE FUNCTION fn_update_timestamp()
                RETURNS trigger
                LANGUAGE plpgsql
                AS $$
                BEGIN
                    NEW.created_at = CURRENT_TIMESTAMP;
                    RETURN NEW;
                END;
                $$;
                """
            )
        ]

        // Sequences
        let sequences = [
            SequenceMetadata(schemaName: "public", name: "users_id_seq", dataType: "integer", startValue: 1, currentValue: 6),
            SequenceMetadata(schemaName: "public", name: "products_id_seq", dataType: "integer", startValue: 1, currentValue: 5),
            SequenceMetadata(schemaName: "public", name: "orders_id_seq", dataType: "integer", startValue: 1, currentValue: 5),
            SequenceMetadata(schemaName: "public", name: "audit_events_10m_id_seq", dataType: "bigint", startValue: 1, currentValue: 10_000_001)
        ]

        let metadata = DatabaseMetadata(
            databaseName: "postgres_production",
            schemas: ["public", "analytics", "audit"],
            tables: [usersTable, productsTable, ordersTable, audit10mTable],
            views: [activeCustView, orderSummaryMatView],
            routines: routines,
            sequences: sequences,
            indexes: indexes,
            constraints: constraints,
            triggers: triggers
        )

        // Seed Rows
        let now = Date()
        let userRows: [DataRow] = [
            DataRow(values: [
                "id": .integer(1),
                "email": .string("emilio@personal.io"),
                "name": .string("Emilio Hernandez"),
                "role": .string("owner"),
                "status": .string("active"),
                "is_verified": .boolean(true),
                "balance": .double(12450.50),
                "profile_data": .json("{\"theme\": \"dark\", \"notifications\": true}"),
                "created_at": .date(now.addingTimeInterval(-86400 * 30))
            ]),
            DataRow(values: [
                "id": .integer(2),
                "email": .string("sarah.connor@cyberdyne.org"),
                "name": .string("Sarah Connor"),
                "role": .string("admin"),
                "status": .string("active"),
                "is_verified": .boolean(true),
                "balance": .double(8420.00),
                "profile_data": .json("{\"clearance\": 5, \"department\": \"Security\"}"),
                "created_at": .date(now.addingTimeInterval(-86400 * 25))
            ]),
            DataRow(values: [
                "id": .integer(3),
                "email": .string("elena.rostova@acme.dev"),
                "name": .string("Elena Rostova"),
                "role": .string("developer"),
                "status": .string("active"),
                "is_verified": .boolean(true),
                "balance": .double(320.15),
                "profile_data": .json("{\"team\": \"Core-API\", \"language\": \"Swift\"}"),
                "created_at": .date(now.addingTimeInterval(-86400 * 18))
            ]),
            DataRow(values: [
                "id": .integer(4),
                "email": .string("marcus.aurelius@stoic.rome"),
                "name": .string("Marcus Aurelius"),
                "role": .string("member"),
                "status": .string("suspended"),
                "is_verified": .boolean(false),
                "balance": .double(0.00),
                "profile_data": .null,
                "created_at": .date(now.addingTimeInterval(-86400 * 10))
            ]),
            DataRow(values: [
                "id": .integer(5),
                "email": .string("ada.lovelace@engines.co"),
                "name": .string("Ada Lovelace"),
                "role": .string("architect"),
                "status": .string("active"),
                "is_verified": .boolean(true),
                "balance": .double(99500.00),
                "profile_data": .json("{\"specialty\": \"Analytical Engines\"}"),
                "created_at": .date(now.addingTimeInterval(-86400 * 5))
            ])
        ]

        let productRows: [DataRow] = [
            DataRow(values: [
                "id": .integer(1),
                "sku": .string("PRD-APL-M3M"),
                "title": .string("MacBook Pro 16\" M3 Max 64GB"),
                "price": .double(3499.00),
                "stock_quantity": .integer(18),
                "is_active": .boolean(true),
                "tags": .array([.string("apple"), .string("laptop"), .string("pro")]),
                "created_at": .date(now.addingTimeInterval(-86400 * 60))
            ]),
            DataRow(values: [
                "id": .integer(2),
                "sku": .string("PRD-MON-4K27"),
                "title": .string("Studio Display 27\" 5K Nano-texture"),
                "price": .double(1899.00),
                "stock_quantity": .integer(7),
                "is_active": .boolean(true),
                "tags": .array([.string("monitor"), .string("5k")]),
                "created_at": .date(now.addingTimeInterval(-86400 * 45))
            ]),
            DataRow(values: [
                "id": .integer(3),
                "sku": .string("PRD-KBD-MECH"),
                "title": .string("Keychron Q1 Pro Wireless Mechanical"),
                "price": .double(198.50),
                "stock_quantity": .integer(42),
                "is_active": .boolean(true),
                "tags": .array([.string("keyboard"), .string("wireless")]),
                "created_at": .date(now.addingTimeInterval(-86400 * 30))
            ]),
            DataRow(values: [
                "id": .integer(4),
                "sku": .string("PRD-MSE-MXM"),
                "title": .string("Logitech MX Master 3S Graphite"),
                "price": .double(99.99),
                "stock_quantity": .integer(85),
                "is_active": .boolean(true),
                "tags": .array([.string("mouse"), .string("ergonomic")]),
                "created_at": .date(now.addingTimeInterval(-86400 * 20))
            ])
        ]

        let orderRows: [DataRow] = [
            DataRow(values: [
                "id": .integer(1),
                "user_id": .integer(1),
                "order_number": .string("ORD-2026-9041"),
                "total_amount": .double(3697.50),
                "status": .string("delivered"),
                "shipping_info": .json("{\"carrier\": \"FedEx\", \"tracking\": \"78129034881\"}"),
                "placed_at": .date(now.addingTimeInterval(-86400 * 12))
            ]),
            DataRow(values: [
                "id": .integer(2),
                "user_id": .integer(3),
                "order_number": .string("ORD-2026-9042"),
                "total_amount": .double(198.50),
                "status": .string("shipped"),
                "shipping_info": .json("{\"carrier\": \"UPS\", \"tracking\": \"1Z9999999999999999\"}"),
                "placed_at": .date(now.addingTimeInterval(-86400 * 4))
            ]),
            DataRow(values: [
                "id": .integer(3),
                "user_id": .integer(2),
                "order_number": .string("ORD-2026-9043"),
                "total_amount": .double(1899.00),
                "status": .string("processing"),
                "shipping_info": .null,
                "placed_at": .date(now.addingTimeInterval(-86400 * 1))
            ])
        ]

        return (metadata, [
            "users": userRows,
            "products": productRows,
            "orders": orderRows,
            "v_active_customers": userRows,
            "mv_order_summaries": orderRows
        ])
    }

    public static func makeMySQLSample() -> (DatabaseMetadata, [String: [DataRow]]) {
        let custCols: [ColumnDefinition] = [
            ColumnDefinition(name: "customer_id", ordinalPosition: 1, dataTypeName: "int(11)", isPrimaryKey: true, isNullable: false),
            ColumnDefinition(name: "company_name", ordinalPosition: 2, dataTypeName: "varchar(150)", isNullable: false),
            ColumnDefinition(name: "contact_email", ordinalPosition: 3, dataTypeName: "varchar(100)", isNullable: false),
            ColumnDefinition(name: "tier", ordinalPosition: 4, dataTypeName: "enum('Standard','Enterprise')", isNullable: false, defaultValue: "'Standard'"),
            ColumnDefinition(name: "monthly_spend", ordinalPosition: 5, dataTypeName: "decimal(12,2)", isNullable: false, defaultValue: "0.00"),
            ColumnDefinition(name: "created_at", ordinalPosition: 6, dataTypeName: "datetime", isNullable: false, defaultValue: "CURRENT_TIMESTAMP")
        ]

        let customersTable = TableMetadata(
            schemaName: "shop_staging",
            name: "customers",
            type: .table,
            estimatedRows: 3,
            columns: custCols,
            primaryKeyColumnNames: ["customer_id"]
        )

        let auditCols: [ColumnDefinition] = [
            ColumnDefinition(name: "id", ordinalPosition: 1, dataTypeName: "bigint", isPrimaryKey: true, isNullable: false),
            ColumnDefinition(name: "event_uuid", ordinalPosition: 2, dataTypeName: "varchar(36)", isNullable: false),
            ColumnDefinition(name: "user_id", ordinalPosition: 3, dataTypeName: "int", isNullable: false),
            ColumnDefinition(name: "action", ordinalPosition: 4, dataTypeName: "varchar(80)", isNullable: false),
            ColumnDefinition(name: "status_code", ordinalPosition: 5, dataTypeName: "int", isNullable: false, defaultValue: "200"),
            ColumnDefinition(name: "ip_address", ordinalPosition: 6, dataTypeName: "varchar(45)", isNullable: false),
            ColumnDefinition(name: "duration_ms", ordinalPosition: 7, dataTypeName: "decimal(8,2)", isNullable: false),
            ColumnDefinition(name: "payload", ordinalPosition: 8, dataTypeName: "json", isNullable: true),
            ColumnDefinition(name: "created_at", ordinalPosition: 9, dataTypeName: "datetime", isNullable: false, defaultValue: "CURRENT_TIMESTAMP")
        ]

        let audit10mTable = TableMetadata(
            schemaName: "shop_staging",
            name: "audit_events_10m",
            type: .table,
            estimatedRows: 10_000_000,
            sizeBytes: 2_147_483_648,
            comment: "Audit events log with 10,000,000 rows (virtualized O(1) paging)",
            columns: auditCols,
            primaryKeyColumnNames: ["id"]
        )

        let routines = [
            RoutineMetadata(
                schemaName: "shop_staging",
                name: "sp_process_daily_settlement",
                routineType: .procedure,
                returnType: "void",
                arguments: "IN p_settlement_date DATE, OUT p_processed_count INT",
                language: "SQL",
                definition: """
                DELIMITER //
                CREATE PROCEDURE sp_process_daily_settlement(IN p_settlement_date DATE, OUT p_processed_count INT)
                BEGIN
                    SELECT COUNT(*) INTO p_processed_count FROM customers WHERE created_at <= p_settlement_date;
                END //
                DELIMITER ;
                """
            ),
            RoutineMetadata(
                schemaName: "shop_staging",
                name: "sp_cleanup_expired_sessions",
                routineType: .procedure,
                returnType: "void",
                arguments: "IN p_max_age_hours INT",
                language: "SQL",
                definition: """
                DELIMITER //
                CREATE PROCEDURE sp_cleanup_expired_sessions(IN p_max_age_hours INT)
                BEGIN
                    DELETE FROM audit_events_10m WHERE created_at < DATE_SUB(NOW(), INTERVAL p_max_age_hours HOUR) LIMIT 10000;
                END //
                DELIMITER ;
                """
            ),
            RoutineMetadata(
                schemaName: "shop_staging",
                name: "fn_calculate_margin",
                routineType: .function,
                returnType: "decimal(10,2)",
                arguments: "p_cost DECIMAL(12,2), p_revenue DECIMAL(12,2)",
                language: "SQL",
                definition: """
                CREATE FUNCTION fn_calculate_margin(p_cost DECIMAL(12,2), p_revenue DECIMAL(12,2))
                RETURNS DECIMAL(10,2)
                DETERMINISTIC
                BEGIN
                    RETURN (p_revenue - p_cost) / p_revenue * 100.0;
                END;
                """
            )
        ]

        let metadata = DatabaseMetadata(
            databaseName: "shop_staging",
            schemas: ["shop_staging"],
            tables: [customersTable, audit10mTable],
            views: [],
            routines: routines,
            indexes: [
                IndexMetadata(name: "PRIMARY", schemaName: "shop_staging", tableName: "customers", isUnique: true, isPrimary: true, method: "BTREE", columnNames: ["customer_id"], definition: "PRIMARY KEY (customer_id)"),
                IndexMetadata(name: "PRIMARY", schemaName: "shop_staging", tableName: "audit_events_10m", isUnique: true, isPrimary: true, method: "BTREE", columnNames: ["id"], definition: "PRIMARY KEY (id)")
            ],
            constraints: [
                ConstraintMetadata(name: "PRIMARY", schemaName: "shop_staging", tableName: "customers", type: .primaryKey, definition: "PRIMARY KEY (customer_id)"),
                ConstraintMetadata(name: "PRIMARY", schemaName: "shop_staging", tableName: "audit_events_10m", type: .primaryKey, definition: "PRIMARY KEY (id)")
            ]
        )

        let now = Date()
        let rows = [
            DataRow(values: [
                "customer_id": .integer(101),
                "company_name": .string("Vercel Technologies"),
                "contact_email": .string("billing@vercel.com"),
                "tier": .string("Enterprise"),
                "monthly_spend": .double(45000.00),
                "created_at": .date(now.addingTimeInterval(-86400 * 90))
            ]),
            DataRow(values: [
                "customer_id": .integer(102),
                "company_name": .string("Stripe Inc."),
                "contact_email": .string("infra@stripe.com"),
                "tier": .string("Enterprise"),
                "monthly_spend": .double(128000.00),
                "created_at": .date(now.addingTimeInterval(-86400 * 120))
            ]),
            DataRow(values: [
                "customer_id": .integer(103),
                "company_name": .string("Supabase Inc."),
                "contact_email": .string("ops@supabase.com"),
                "tier": .string("Standard"),
                "monthly_spend": .double(8500.00),
                "created_at": .date(now.addingTimeInterval(-86400 * 40))
            ])
        ]

        return (metadata, ["customers": rows])
    }

    public static func makeMongoSample() -> (DatabaseMetadata, [String: [DataRow]]) {
        let userProfileCols: [ColumnDefinition] = [
            ColumnDefinition(name: "_id", ordinalPosition: 1, dataTypeName: "ObjectId", isPrimaryKey: true, isNullable: false),
            ColumnDefinition(name: "username", ordinalPosition: 2, dataTypeName: "String", isNullable: false),
            ColumnDefinition(name: "email", ordinalPosition: 3, dataTypeName: "String", isNullable: false),
            ColumnDefinition(name: "score", ordinalPosition: 4, dataTypeName: "Double", isNullable: false),
            ColumnDefinition(name: "verified", ordinalPosition: 5, dataTypeName: "Boolean", isNullable: false),
            ColumnDefinition(name: "document", ordinalPosition: 6, dataTypeName: "Document", isNullable: false)
        ]

        let userProfilesColl = TableMetadata(
            schemaName: "ecom_nosql",
            name: "user_profiles",
            type: .collection,
            estimatedRows: 3,
            sizeBytes: 16384,
            columns: userProfileCols,
            primaryKeyColumnNames: ["_id"]
        )

        let eventsColl = TableMetadata(
            schemaName: "ecom_nosql",
            name: "events_stream",
            type: .collection,
            estimatedRows: 4,
            sizeBytes: 24576,
            columns: userProfileCols,
            primaryKeyColumnNames: ["_id"]
        )

        let stats = [
            "user_profiles": MongoCollectionStats(documentCount: 3, avgDocumentSizeBytes: 420.0, totalStorageSizeBytes: 16384, indexCount: 2, totalIndexSizeBytes: 8192),
            "events_stream": MongoCollectionStats(documentCount: 4, avgDocumentSizeBytes: 580.0, totalStorageSizeBytes: 24576, indexCount: 2, totalIndexSizeBytes: 8192)
        ]

        let metadata = DatabaseMetadata(
            databaseName: "ecom_nosql",
            schemas: ["ecom_nosql"],
            tables: [userProfilesColl, eventsColl],
            mongoStats: stats
        )

        let doc1 = """
        {
          "_id": "65b9e110a12fbc9823000001",
          "username": "alex_developer",
          "email": "alex@devhub.net",
          "score": 98.4,
          "verified": true,
          "preferences": {
            "newsletter": false,
            "theme": "tokyo-night",
            "editor": "neovim"
          },
          "skills": ["Rust", "Swift", "Postgres", "MongoDB"],
          "login_count": 142
        }
        """

        let doc2 = """
        {
          "_id": "65b9e110a12fbc9823000002",
          "username": "clara_cloud",
          "email": "clara@awscloud.org",
          "score": 87.2,
          "verified": true,
          "preferences": {
            "newsletter": true,
            "theme": "catppuccin",
            "editor": "vscode"
          },
          "skills": ["Kubernetes", "Terraform", "Go"],
          "login_count": 89
        }
        """

        let rows = [
            DataRow(values: [
                "_id": .objectId("65b9e110a12fbc9823000001"),
                "username": .string("alex_developer"),
                "email": .string("alex@devhub.net"),
                "score": .double(98.4),
                "verified": .boolean(true),
                "document": .json(doc1)
            ]),
            DataRow(values: [
                "_id": .objectId("65b9e110a12fbc9823000002"),
                "username": .string("clara_cloud"),
                "email": .string("clara@awscloud.org"),
                "score": .double(87.2),
                "verified": .boolean(true),
                "document": .json(doc2)
            ])
        ]

        return (metadata, ["user_profiles": rows, "events_stream": rows])
    }

    public static func generateAuditEvents(offset: Int, limit: Int, totalRows: Int = 10_000_000) -> [DataRow] {
        let actions = [
            "auth.login.success", "auth.token.refresh", "user.profile.update",
            "order.checkout.initiated", "order.payment.completed", "api.v1.users.query",
            "system.cache.invalidate", "subscription.renewed", "export.report.generated",
            "security.mfa.verified", "security.password.reset", "webhook.delivery.ok"
        ]
        let statusCodes = [200, 200, 200, 201, 200, 204, 200, 400, 401, 403, 404, 500]
        let baseTime = Date()

        var rows: [DataRow] = []
        rows.reserveCapacity(limit)

        for i in 0..<limit {
            let globalId = offset + i + 1
            if globalId > totalRows { break }

            let actionIdx = (globalId * 7) % actions.count
            let action = actions[actionIdx]
            let statusCode = statusCodes[(globalId * 3) % statusCodes.count]
            let userId = ((globalId * 13) % 5) + 1
            let duration = Double((globalId * 17) % 850) / 10.0 + 1.2
            let ipPart3 = (globalId * 19) % 254 + 1
            let ipPart4 = (globalId * 31) % 254 + 1
            let ip = "192.168.\(ipPart3).\(ipPart4)"
            let uuid = String(format: "%08x-78b1-4c32-a5e9-%012x", globalId, (globalId * 997))
            let time = baseTime.addingTimeInterval(-Double(totalRows - globalId) * 0.8)

            let row = DataRow(values: [
                "id": .integer(Int64(globalId)),
                "event_uuid": .string(uuid),
                "user_id": .integer(Int64(userId)),
                "action": .string(action),
                "status_code": .integer(Int64(statusCode)),
                "ip_address": .string(ip),
                "duration_ms": .double(duration),
                "payload": .json("{\"batch\": \(globalId / 100), \"retry\": \(globalId % 3 == 0)}"),
                "created_at": .date(time)
            ])
            rows.append(row)
        }
        return rows
    }
}

