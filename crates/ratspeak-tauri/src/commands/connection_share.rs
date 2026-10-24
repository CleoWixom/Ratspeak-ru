@@
-        let slot = match key.as_ref() {
-            "v" => &mut raw.version,
-            "kind" => &mut raw.kind,
-            "host" => &mut raw.host,
-            "port" => &mut raw.port,
-            "freq" => &mut raw.freq,
-            "bw" => &mut raw.bw,
-            "sf" => &mut raw.sf,
-            "cr" => &mut raw.cr,
-            "tx" => &mut raw.tx,
-            "name" => &mut raw.name,
-            // Includes every secret-bearing spelling (`ifac`, `passphrase`,
-            // `key`, ...): an unknown field rejects the whole link.
-            _ => return Err("Connection share contains an unsupported field".into()),
-        };
+        let slot = match key.as_ref() {
+            "v" => &mut raw.version,
+            "kind" => &mut raw.kind,
+            "host" => &mut raw.host,
+            "port" => &mut raw.port,
+            "freq" => &mut raw.freq,
+            "bw" => &mut raw.bw,
+            "sf" => &mut raw.sf,
+            "cr" => &mut raw.cr,
+            "tx" => &mut raw.tx,
+            "name" => &mut raw.name,
+            // Includes every secret-bearing spelling (`ifac`, `passphrase`,
+            // `key`, ...): an unknown field rejects the whole link.
+            _ => return Err("Connection share contains an unsupported field".into()),
+        };
