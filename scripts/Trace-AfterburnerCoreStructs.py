from __future__ import annotations

import argparse
import json
import textwrap
import time
from pathlib import Path

import frida


def build_script() -> str:
    return textwrap.dedent(
        r"""
        const OFFSETS = {
          vfCurveSetCurve: ptr('0x00088410'),
          helperCoreStage: ptr('0x00009110'),
          helperCurveStage: ptr('0x0000B9E0'),
          helperCommit: ptr('0x00029D00'),
          callCoreStageA: ptr('0x000886ED'),
          callCoreStageB: ptr('0x0008877F'),
          callCurveStage: ptr('0x00088996'),
        };

        const GLOBAL_RVAS = {
          pendingCount5c7c: 0x000E5C7C,
          pendingCount5cc4: 0x000E5CC4,
          mode5c84: 0x000E5C84,
          mode5c78: 0x000E5C78,
        };

        const MAX_EVENTS = {
          vfCurveSetCurve: 8,
          helperCoreStage: 12,
          helperCurveStage: 12,
          helperCommit: 6,
          callCoreStageA: 8,
          callCoreStageB: 8,
          callCurveStage: 8,
        };

        const counts = {
          vfCurveSetCurve: 0,
          helperCoreStage: 0,
          helperCurveStage: 0,
          helperCommit: 0,
          callCoreStageA: 0,
          callCoreStageB: 0,
          callCurveStage: 0,
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

        function dumpStruct(address, length, dwordCount) {
          if (address.isNull()) return null;
          const bytes = safeReadBytes(address, length);
          return {
            address: ptrHex(address),
            length,
            headerU32: dwordsFromBytes(bytes, dwordCount),
            bytes,
          };
        }

        function hookRthal(module) {
          const base = module.base;
          send({
            type: 'rthal-hook-ready',
            module: module.name,
            base: ptrHex(base),
          });

          const vfCurveSetCurve = base.add(OFFSETS.vfCurveSetCurve);
          const helperCoreStage = base.add(OFFSETS.helperCoreStage);
          const helperCurveStage = base.add(OFFSETS.helperCurveStage);
          const helperCommit = base.add(OFFSETS.helperCommit);
          const callCoreStageA = base.add(OFFSETS.callCoreStageA);
          const callCoreStageB = base.add(OFFSETS.callCoreStageB);
          const callCurveStage = base.add(OFFSETS.callCurveStage);

          Interceptor.attach(vfCurveSetCurve, {
            onEnter(args) {
              if (counts.vfCurveSetCurve >= MAX_EVENTS.vfCurveSetCurve) return;
              counts.vfCurveSetCurve += 1;
              const thisPtr = this.context.ecx;
              const descPtr = args[0];
              const payload = {
                type: 'vfcurve-setcurve-enter',
                caller: describeAddress(this.returnAddress),
                thisPtr: ptrHex(thisPtr),
                descPtr: ptrHex(descPtr),
                stagedCoreMultiplier: safeReadS32(thisPtr.add(0x1604)),
                stagedCurveFlag1600: safeReadU32(thisPtr.add(0x1600)),
                field1538: safeReadU32(thisPtr.add(0x1538)),
                descPreview: dumpStruct(descPtr, 0x80, 16),
              };
              send(payload);
            }
          });

          function emitCallsiteBuffer(kind, context, offset, length, dwordCount) {
            const ebp = context.ebp;
            const structPtr = ebp.add(offset);
            send({
              type: kind,
              caller: describeAddress(context.eip),
              ebp: ptrHex(ebp),
              struct: dumpStruct(structPtr, length, dwordCount),
            });
          }

          Interceptor.attach(callCoreStageA, {
            onEnter(args) {
              if (counts.callCoreStageA >= MAX_EVENTS.callCoreStageA) return;
              counts.callCoreStageA += 1;
              emitCallsiteBuffer('core-stage-callsite-a', this.context, -0x310, 0x30c, 24);
            }
          });

          Interceptor.attach(callCoreStageB, {
            onEnter(args) {
              if (counts.callCoreStageB >= MAX_EVENTS.callCoreStageB) return;
              counts.callCoreStageB += 1;
              emitCallsiteBuffer('core-stage-callsite-b', this.context, -0x310, 0x30c, 24);
            }
          });

          Interceptor.attach(callCurveStage, {
            onEnter(args) {
              if (counts.callCurveStage >= MAX_EVENTS.callCurveStage) return;
              counts.callCurveStage += 1;
              emitCallsiteBuffer('curve-stage-callsite', this.context, -0x6624, 0x180, 48);
            }
          });

          Interceptor.attach(helperCoreStage, {
            onEnter(args) {
              if (counts.helperCoreStage >= MAX_EVENTS.helperCoreStage) return;
              counts.helperCoreStage += 1;
              const handle = args[0];
              const structPtr = args[1];
              send({
                type: 'core-stage-enter',
                caller: describeAddress(this.returnAddress),
                handle: ptrHex(handle),
                struct: dumpStruct(structPtr, 0x30c, 20),
              });
            }
          });

          Interceptor.attach(helperCurveStage, {
            onEnter(args) {
              if (counts.helperCurveStage >= MAX_EVENTS.helperCurveStage) return;
              counts.helperCurveStage += 1;
              const handle = args[0];
              const structPtr = args[1];
              const version = safeReadU32(structPtr);
              let length = 0x80;
              if (version === 0x12420 || version === 0x22420) {
                length = 0x140;
              }
              send({
                type: 'curve-stage-enter',
                caller: describeAddress(this.returnAddress),
                handle: ptrHex(handle),
                version,
                struct: dumpStruct(structPtr, length, 24),
              });
            }
          });

          Interceptor.attach(helperCommit, {
            onEnter(args) {
              if (counts.helperCommit >= MAX_EVENTS.helperCommit) return;
              counts.helperCommit += 1;
              send({
                type: 'commit-enter',
                caller: describeAddress(this.returnAddress),
                state: {
                  pendingCount5c7c: safeReadU32(base.add(GLOBAL_RVAS.pendingCount5c7c)),
                  pendingCount5cc4: safeReadU32(base.add(GLOBAL_RVAS.pendingCount5cc4)),
                  mode5c84: safeReadU32(base.add(GLOBAL_RVAS.mode5c84)),
                  mode5c78: safeReadU32(base.add(GLOBAL_RVAS.mode5c78)),
                },
              });
            }
          });
        }

        function tryHook() {
          try {
            const module = Process.getModuleByName('RTHAL.dll');
            hookRthal(module);
            return true;
          } catch (_) {
            return false;
          }
        }

        send({ type: 'script-loaded' });

        if (!tryHook()) {
          const kernel32 = Process.getModuleByName('kernel32.dll');
          const loadLibraryW = kernel32.getExportByName('LoadLibraryW');
          const loadLibraryA = kernel32.getExportByName('LoadLibraryA');

          function check() {
            tryHook();
          }

          Interceptor.attach(loadLibraryW, {
            onLeave(_) { check(); }
          });
          Interceptor.attach(loadLibraryA, {
            onLeave(_) { check(); }
          });

          const timer = setInterval(function() {
            if (tryHook()) {
              clearInterval(timer);
            }
          }, 100);
        }
        """
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Trace MSI Afterburner core-structure payloads for private NVAPI helper calls."
    )
    parser.add_argument("--process", default="MSIAfterburner.exe", help="Process name to attach to")
    parser.add_argument("--pid", type=int, help="Process ID to attach to")
    parser.add_argument("--spawn", action="store_true", help="Spawn the target process instead of attaching")
    parser.add_argument("--seconds", type=int, default=90, help="How long to trace before detaching")
    parser.add_argument(
        "--out",
        default=r"C:\Users\noah\Desktop\afterburner-core-structs.json",
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
        "events": events,
    }
    Path(args.out).write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(f"Wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
