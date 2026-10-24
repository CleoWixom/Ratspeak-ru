@@
-        let slot = match key.as_ref() {
-            "v" => &mut version,
-            "hub" => &mut hub,
-            "room" => &mut room,
-            _ => return Err("Channel share contains an unsupported field".into()),
-        };
+        let slot = match key.as_ref() {
+            "v" => &mut version,
+            "hub" => &mut hub,
+            "room" => &mut room,
+            _ => return Err("Channel share contains an unsupported field".into()),
+        };
