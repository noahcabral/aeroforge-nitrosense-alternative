from __future__ import annotations

import argparse
import json
import textwrap
import time
from pathlib import Path

import frida


PUBLIC_IDS = {
    0x0150E828: "NvAPI_Initialize",
    0xD22BDD7E: "NvAPI_Unload",
    0x6C2D048C: "NvAPI_GetErrorMessage",
    0xE5AC921F: "NvAPI_EnumPhysicalGPUs",
    0xCEEE8E9F: "NvAPI_GPU_GetFullName",
    0x927DA4F6: "NvAPI_GPU_GetCurrentPstate",
    0x6FF81213: "NvAPI_GPU_GetPstates20",
    0x0F4DAE6B: "NvAPI_GPU_SetPstates20",
}


def build_script() -> str:
    return textwrap.dedent(
        r"""
        const publicIds = {
          0x0150E828: "NvAPI_Initialize",
          0xD22BDD7E: "NvAPI_Unload",
          0x6C2D048C: "NvAPI_GetErrorMessage",
          0xE5AC921F: "NvAPI_EnumPhysicalGPUs",
          0xCEEE8E9F: "NvAPI_GPU_GetFullName",
          0x927DA4F6: "NvAPI_GPU_GetCurrentPstate",
          0x6FF81213: "NvAPI_GPU_GetPstates20",
          0x0F4DAE6B: "NvAPI_GPU_SetPstates20",
        };

        const seenFunctions = new Set();
        const seenHooks = new Set();

        function ptrHex(value) {
          return ptr(value).toString();
        }

        function readAscii(value) {
          if (value.isNull()) return null;
          try {
            return Memory.readCString(value);
          } catch (_) {
            return null;
          }
        }

        function readUtf16(value) {
          if (value.isNull()) return null;
          try {
            return Memory.readUtf16String(value);
          } catch (_) {
            return null;
          }
        }

        function describeAddress(value) {
          try {
            const symbol = DebugSymbol.fromAddress(value);
            return {
              address: ptrHex(value),
              name: symbol ? symbol.name || null : null,
              module: symbol && symbol.moduleName ? symbol.moduleName : null,
            };
          } catch (_) {
            return {
              address: ptrHex(value),
              name: null,
              module: null,
            };
          }
        }

        function includeModule(name) {
          const lower = name.toLowerCase();
          return (
            lower.indexOf("nv") !== -1 ||
            lower.indexOf("msi") !== -1 ||
            lower.indexOf("rthal") !== -1 ||
            lower.indexOf("rtcore") !== -1
          );
        }

        function moduleSnapshot() {
          return Process.enumerateModules()
            .filter(m => includeModule(m.name))
            .map(m => ({
              name: m.name,
              base: ptrHex(m.base),
              size: m.size,
              path: m.path,
            }));
        }

        function emitLoad(kind, requested, retval) {
          const requestedLower = requested ? requested.toLowerCase() : "";
          if (
            requestedLower.indexOf("nvapi") === -1 &&
            requestedLower.indexOf("rtcore") === -1 &&
            requestedLower.indexOf("rthal") === -1
          ) {
            return;
          }
          send({
            type: kind,
            requested,
            returned: ptrHex(retval),
            modules: moduleSnapshot(),
          });
        }

        send({ type: "script-loaded" });
        send({ type: "module-snapshot", modules: moduleSnapshot() });

        const kernel32 = Process.getModuleByName("kernel32.dll");

        const loadLibraryA = kernel32.getExportByName("LoadLibraryA");
        Interceptor.attach(loadLibraryA, {
          onEnter(args) {
            this.requested = readAscii(args[0]);
          },
          onLeave(retval) {
            emitLoad("loadlibrarya", this.requested, retval);
          }
        });

        const loadLibraryW = kernel32.getExportByName("LoadLibraryW");
        Interceptor.attach(loadLibraryW, {
          onEnter(args) {
            this.requested = readUtf16(args[0]);
          },
          onLeave(retval) {
            emitLoad("loadlibraryw", this.requested, retval);
          }
        });

        const getProcAddress = kernel32.getExportByName("GetProcAddress");
        Interceptor.attach(getProcAddress, {
          onEnter(args) {
            this.name = readAscii(args[1]);
            this.module = describeAddress(args[0]);
          },
          onLeave(retval) {
            if (!this.name) return;
            const lower = this.name.toLowerCase();
            if (
              lower.indexOf("nvapi") === -1 &&
              lower.indexOf("rtcore") === -1 &&
              lower.indexOf("rthal") === -1
            ) {
              return;
            }
            send({
              type: "getproc",
              name: this.name,
              module: this.module,
              returned: ptrHex(retval),
            });
          }
        });

        function hookResolvedFunction(idValue, fnPtr) {
          if (fnPtr.isNull()) return;
          const resolvedTarget = ptr(fnPtr.toString());
          const key = ptrHex(resolvedTarget);
          if (seenFunctions.has(key)) return;
          seenFunctions.add(key);
          const numericId = idValue >>> 0;
          Interceptor.attach(resolvedTarget, {
            onEnter(args) {
              this.caller = describeAddress(this.returnAddress);
              send({
                type: "nvapi-call",
                interfaceId: "0x" + numericId.toString(16),
                interfaceName: publicIds[numericId] || null,
                target: describeAddress(resolvedTarget),
                caller: this.caller,
                args: [ptrHex(args[0]), ptrHex(args[1]), ptrHex(args[2]), ptrHex(args[3])],
              });
            },
            onLeave(retval) {
              send({
                type: "nvapi-return",
                interfaceId: "0x" + numericId.toString(16),
                interfaceName: publicIds[numericId] || null,
                target: describeAddress(resolvedTarget),
                caller: this.caller,
                retval: ptrHex(retval),
              });
            }
          });
        }

        function hookQueryInterface(address) {
          const key = ptrHex(address);
          if (seenHooks.has(key)) return;
          seenHooks.add(key);
          send({
            type: "hook",
            name: "nvapi_QueryInterface",
            address: ptrHex(address),
            symbol: describeAddress(address),
          });
          Interceptor.attach(address, {
            onEnter(args) {
              this.interfaceId = args[0].toUInt32();
              this.caller = describeAddress(this.returnAddress);
            },
            onLeave(retval) {
              const idValue = this.interfaceId >>> 0;
              send({
                type: "query-interface",
                interfaceId: "0x" + idValue.toString(16),
                interfaceName: publicIds[idValue] || null,
                caller: this.caller,
                returned: ptrHex(retval),
              });
              hookResolvedFunction(idValue, retval);
            }
          });
        }

        function waitForNvapi() {
          const candidates = ["nvapi64.dll", "nvapi.dll"];
          const timer = setInterval(function() {
            let nvapiModule = null;
            for (const name of candidates) {
              try {
                nvapiModule = Process.getModuleByName(name);
                break;
              } catch (_) {
              }
            }
            if (!nvapiModule) return;

            let query = null;
            try {
              query = nvapiModule.getExportByName("nvapi_QueryInterface");
            } catch (_) {
              query = null;
            }

            if (!query) return;
            clearInterval(timer);
            hookQueryInterface(query);
          }, 100);
        }

        waitForNvapi();
        """
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="Trace MSI Afterburner NVAPI runtime behavior via Frida.")
    parser.add_argument("--process", default="MSIAfterburner.exe", help="Process name to attach to")
    parser.add_argument("--pid", type=int, help="Process ID to attach to")
    parser.add_argument("--spawn", action="store_true", help="Spawn the target process instead of attaching")
    parser.add_argument("--seconds", type=int, default=25, help="How long to trace before detaching")
    parser.add_argument(
        "--out",
        default=r"C:\Users\noah\Desktop\afterburner-nvapi-runtime.json",
        help="Path to write trace JSON",
    )
    args = parser.parse_args()

    device = frida.get_local_device()
    spawned_pid: int | None = None

    if args.spawn:
        spawned_pid = device.spawn([args.process])
        session = device.attach(spawned_pid)
        attached_pid = spawned_pid
    elif args.pid is not None:
        session = device.attach(args.pid)
        attached_pid = args.pid
    else:
        session = device.attach(args.process)
        try:
            attached_pid = session.pid  # type: ignore[attr-defined]
        except Exception:
            attached_pid = None

    events: list[dict[str, object]] = []

    def on_message(message, data):
        if message["type"] == "send":
            payload = message["payload"]
            payload["timestamp"] = time.time()
            payload["sourcePid"] = attached_pid
            events.append(payload)
            print(json.dumps(payload, ensure_ascii=True))
        else:
            wrapped = {
                "type": "frida-message",
                "message": message,
                "timestamp": time.time(),
                "sourcePid": attached_pid,
            }
            events.append(wrapped)
            print(json.dumps(wrapped, ensure_ascii=True))

    script = session.create_script(build_script())
    script.on("message", on_message)
    script.load()

    try:
        if spawned_pid is not None:
            device.resume(spawned_pid)
        time.sleep(args.seconds)
    finally:
        try:
            session.detach()
        except Exception:
            pass

    result = {
        "process": args.process,
        "pid": args.pid,
        "attachedPid": attached_pid,
        "spawned": bool(args.spawn),
        "durationSeconds": args.seconds,
        "publicIds": {hex(key): value for key, value in PUBLIC_IDS.items()},
        "events": events,
    }
    Path(args.out).write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(f"Wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
