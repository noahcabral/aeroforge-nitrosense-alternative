from __future__ import annotations

import argparse
import json
import textwrap
import time
from pathlib import Path

import frida


TARGET_IDS = {
    0x0733E009: "MemoryPrivateStageA",
    0x39442CFB: "MemoryPrivateStageB",
    0xD7C61344: "MemoryPrivateCommit",
    0x0F4DAE6B: "NvAPI_GPU_SetPstates20",
}


def build_script() -> str:
    return textwrap.dedent(
        r"""
        const TARGET_IDS = {
          0x0733E009: 'MemoryPrivateStageA',
          0x39442CFB: 'MemoryPrivateStageB',
          0xD7C61344: 'MemoryPrivateCommit',
          0x0F4DAE6B: 'NvAPI_GPU_SetPstates20',
        };

        const WRAPPER_OFFSETS = {
          stageAWrapper: ptr('0x0000B9E0'),
          stageBWrapper: ptr('0x00009110'),
          commitWrapper: ptr('0x00029D60'),
        };

        function ptrHex(value) {
          return ptr(value).toString();
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

        function safeReadU32(address) {
          try {
            return Memory.readU32(address);
          } catch (_) {
            return null;
          }
        }

        function safeReadS32(address) {
          try {
            return Memory.readS32(address);
          } catch (_) {
            return null;
          }
        }

        function safeReadBytes(address, length) {
          try {
            const out = [];
            let cursor = ptr(address);
            for (let i = 0; i < length; i++) {
              try {
                out.push(Memory.readU8(cursor.add(i)));
              } catch (_) {
                break;
              }
            }
            return out.length ? out : null;
          } catch (_) {
            return null;
          }
        }

        function dwordsFromBytes(bytes, count) {
          if (!bytes) return null;
          const view = new DataView(Uint8Array.from(bytes).buffer);
          const out = [];
          for (let i = 0; i < count; i++) {
            const offset = i * 4;
            if (offset + 4 > view.byteLength) break;
            out.push(view.getUint32(offset, true));
          }
          return out;
        }

        function dumpBuffer(address, length, dwordCount) {
          if (address.isNull()) return null;
          const range = Process.findRangeByAddress(address);
          if (!range) {
            return {
              address: ptrHex(address),
              length,
              range: null,
              headerU32: null,
              bytes: null,
            };
          }
          const bytes = safeReadBytes(address, length);
          return {
            address: ptrHex(address),
            length,
            range: {
              base: ptrHex(range.base),
              size: range.size,
              protection: range.protection,
              file: range.file ? range.file.path : null,
            },
            headerU32: dwordsFromBytes(bytes, dwordCount),
            bytes,
          };
        }

        function dumpStack(context, length, dwordCount) {
          return dumpBuffer(context.esp, length, dwordCount);
        }

        function dumpBacktrace(context) {
          try {
            return Thread.backtrace(context, Backtracer.ACCURATE)
              .slice(0, 8)
              .map(describeAddress);
          } catch (_) {
            return null;
          }
        }

        const seenResolvedTargets = new Set();
        let wrappersHooked = false;

        function hookWrappers() {
          if (wrappersHooked) return;
          const module = Process.getModuleByName('RTHAL.dll');
          const base = module.base;

          function attachWrapper(name, offset) {
            const target = base.add(offset);
            Interceptor.attach(target, {
              onEnter(args) {
                send({
                  type: 'memory-wrapper-enter',
                  wrapper: name,
                  target: describeAddress(target),
                  caller: describeAddress(this.returnAddress),
                  backtrace: dumpBacktrace(this.context),
                  args: [
                    ptrHex(args[0]),
                    ptrHex(args[1]),
                    ptrHex(args[2]),
                    ptrHex(args[3]),
                  ],
                  arg0Preview: dumpBuffer(args[0], 0x180, 24),
                  arg1Preview: dumpBuffer(args[1], 0x180, 24),
                  arg2Preview: dumpBuffer(args[2], 0x180, 24),
                  stackPreview: dumpStack(this.context, 0x120, 32),
                });
              }
            });
          }

          attachWrapper('stageAWrapper', WRAPPER_OFFSETS.stageAWrapper);
          attachWrapper('stageBWrapper', WRAPPER_OFFSETS.stageBWrapper);
          attachWrapper('commitWrapper', WRAPPER_OFFSETS.commitWrapper);
          wrappersHooked = true;
        }

        function hookResolvedFunction(interfaceId, fnPtr) {
          if (fnPtr.isNull()) return;
          const resolvedTarget = ptr(fnPtr.toString());
          const key = ptrHex(resolvedTarget);
          if (seenResolvedTargets.has(key)) return;
          seenResolvedTargets.add(key);

          Interceptor.attach(resolvedTarget, {
            onEnter(args) {
              this.caller = describeAddress(this.returnAddress);
              this.interfaceId = interfaceId >>> 0;
              send({
                type: 'memory-helper-enter',
                helperId: '0x' + this.interfaceId.toString(16),
                helperName: TARGET_IDS[this.interfaceId] || null,
                caller: this.caller,
                target: describeAddress(resolvedTarget),
                args: [
                  ptrHex(args[0]),
                  ptrHex(args[1]),
                  ptrHex(args[2]),
                  ptrHex(args[3]),
                ],
                arg0U32: safeReadU32(args[0]),
                arg1Preview: dumpBuffer(args[1], 0x180, 32),
                arg2Preview: dumpBuffer(args[2], 0x180, 32),
                arg3U32: safeReadU32(args[3]),
                arg1S32_10: safeReadS32(args[1].add(0x10)),
                arg1S32_14: safeReadS32(args[1].add(0x14)),
                arg1S32_18: safeReadS32(args[1].add(0x18)),
                arg2S32_10: safeReadS32(args[2].add(0x10)),
                arg2S32_14: safeReadS32(args[2].add(0x14)),
                arg2S32_18: safeReadS32(args[2].add(0x18)),
              });
            },
            onLeave(retval) {
              send({
                type: 'memory-helper-return',
                helperId: '0x' + this.interfaceId.toString(16),
                helperName: TARGET_IDS[this.interfaceId] || null,
                caller: this.caller,
                target: describeAddress(resolvedTarget),
                retval: ptrHex(retval),
              });
            }
          });
        }

        function hookQueryInterface(address) {
          send({
            type: 'hook',
            name: 'nvapi_QueryInterface',
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
              if (!(idValue in TARGET_IDS)) return;
              send({
                type: 'query-interface',
                interfaceId: '0x' + idValue.toString(16),
                interfaceName: TARGET_IDS[idValue],
                caller: this.caller,
                returned: ptrHex(retval),
              });
              hookResolvedFunction(idValue, retval);
            }
          });
        }

        function waitForNvapi() {
          const timer = setInterval(function() {
            try {
              hookWrappers();
              const nvapiModule = Process.getModuleByName('nvapi.dll');
              const query = nvapiModule.getExportByName('nvapi_QueryInterface');
              clearInterval(timer);
              hookQueryInterface(query);
            } catch (_) {
            }
          }, 250);
        }

        send({ type: 'script-loaded' });
        waitForNvapi();
        """
    )


def collect_trace(
    process_name: str,
    pid: int | None,
    spawn: bool,
    seconds: int,
) -> tuple[list[dict[str, object]], int | None, int | None, bool]:
    device = frida.get_local_device()
    attached_pid: int | None = None
    target_pid: int | None = None
    spawned = False

    if spawn:
        target_pid = device.spawn([process_name])
        session = device.attach(target_pid)
        device.resume(target_pid)
        spawned = True
    else:
        attached_pid = pid or device.get_process(process_name).pid
        session = device.attach(attached_pid)
        target_pid = attached_pid

    events: list[dict[str, object]] = []
    script = session.create_script(build_script())

    def on_message(message, data):
        if message["type"] == "send":
            payload = dict(message["payload"])
            payload.setdefault("timestamp", time.time())
            payload.setdefault("sourcePid", target_pid)
            events.append(payload)
        elif message["type"] == "error":
            events.append(
                {
                    "type": "script-error",
                    "timestamp": time.time(),
                    "sourcePid": target_pid,
                    "description": message.get("description"),
                    "stack": message.get("stack"),
                }
            )

    script.on("message", on_message)
    script.load()

    time.sleep(seconds)

    session.detach()
    return events, target_pid, attached_pid, spawned


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Trace MSI Afterburner memory-OC helper payloads for clean-room reproduction."
    )
    parser.add_argument(
        "--process",
        default="MSIAfterburner.exe",
        help="Process name to attach to",
    )
    parser.add_argument(
        "--pid",
        type=int,
        help="Process ID to attach to",
    )
    parser.add_argument(
        "--spawn",
        action="store_true",
        help="Spawn the target process instead of attaching",
    )
    parser.add_argument(
        "--seconds",
        type=int,
        default=45,
        help="How long to trace before detaching",
    )
    parser.add_argument(
        "--out",
        default=r"C:\Users\noah\Desktop\afterburner-memory-structs-live.json",
        help="Path to write trace JSON",
    )
    args = parser.parse_args()

    events, target_pid, attached_pid, spawned = collect_trace(
        args.process,
        args.pid,
        args.spawn,
        args.seconds,
    )

    output = {
        "process": args.process,
        "pid": target_pid,
        "attachedPid": attached_pid,
        "spawned": spawned,
        "durationSeconds": args.seconds,
        "targetIds": {f"0x{key:08x}": value for key, value in TARGET_IDS.items()},
        "events": events,
    }

    out_path = Path(args.out)
    out_path.write_text(json.dumps(output, indent=2), encoding="utf-8")
    print(f"Wrote {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
