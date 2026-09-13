# UniFFI bindings are called through JNA by name.
-keep class com.sun.jna.** { *; }
-keep class uniffi.soundpush_ffi.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-dontwarn java.awt.**
