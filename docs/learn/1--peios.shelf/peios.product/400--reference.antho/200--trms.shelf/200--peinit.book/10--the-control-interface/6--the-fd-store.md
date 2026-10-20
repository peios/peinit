---
title: The Fd Store
description: Keeping file descriptors across a service's own restart — storing, removing, injecting,  and clearing them.
---

The fd store lets a service keep file descriptors across its own
restart. It pushes them to peinit, peinit holds them, and the new
process gets them back. That is what lets a stateful daemon — a web
server holding a listening socket, say — restart without dropping
connections it has already accepted.

`FdStoreMax` in the definition sets the maximum number of descriptors
peinit will hold. It defaults to **0**, which disables the store
[*fdstore.fdstoremax-defaults-to-zero-and-zero-disables-the-store]: most
services do not need it, and holding descriptors on behalf of a service
that will never ask for them back is pure cost.

## Storing

When an authenticated datagram carries `FDSTORE=1` with descriptors
attached:

1. If `FdStoreMax` is 0, peinit logs the rejection and **closes** the
   descriptor. [*fdstore.a-disabled-store-closes-the-descriptor]
2. If the store already holds `FdStoreMax` entries, peinit logs the
   rejection and closes the descriptor. The existing store is not
   modified — a full store does not evict.
   [*fdstore.a-full-store-does-not-evict]
3. `FDNAME=<name>` names it; an absent or empty name means `stored`.
   [*fdstore.an-unnamed-descriptor-is-named-stored]
4. `FDPOLL=0` marks the descriptor exempt from poll monitoring.

Either rejection emits a `peinit.fd-store.rejected` event carrying the
service, the descriptor's name and the reason (`disabled` or `full`), so
a service whose descriptors are being silently dropped can find out why.
[*fdstore.a-rejection-emits-an-event]

Several descriptors can share a name. One `FDSTORE=1` carrying N
descriptors creates N entries under the one name, each independently
subject to the limit — so the first few fit and the overflow is
<!-- lint-disable-next-line spelling -->
rejected and closed. [*fdstore.several-descriptors-may-share-a-name]

peinit does not monitor stored descriptors. The `FDPOLL` flag is
recorded and nothing reads it, so no stored descriptor is evicted for
becoming invalid. [*fdstore.stored-descriptors-are-never-monitored]

## Removing

`FDSTOREREMOVE=1` with `FDNAME=<name>` removes every descriptor of that
name and closes them. A name matching nothing is a no-op rather than an
error. [*fdstore.a-named-remove-closes-every-descriptor-of-that-name]

`FDSTOREREMOVE=1` **without** a name aborts the whole fd-store step for
that datagram — so a datagram carrying both an unnamed remove and an
`FDSTORE=1` performs neither, and the attached descriptors are dropped
and closed. [*fdstore.an-unnamed-remove-aborts-the-fd-store-step]

## Injecting

When a service restarts, peinit injects the stored descriptors during
the child's pre-exec path (§5.4):

1. They are placed consecutively from `SD_LISTEN_FDS_START` — descriptor
   3 — upward, with close-on-exec cleared: the only sanctioned
   exception to the close-on-exec discipline.
   [*fdstore.descriptors-are-injected-from-descriptor-three-upward]
2. `LISTEN_FDS` is set to the count.
   [*fdstore.listen-fds-is-the-count]
3. `LISTEN_FDNAMES` is set to a **colon-separated** list of names in the
   same order as the descriptor numbers.
   [*fdstore.listen-fdnames-is-colon-separated-and-in-order]
4. The store is cleared. peinit has released them.

All three variables are omitted entirely when the store is empty.
[*fdstore.the-listen-variables-are-omitted-when-the-store-is-empty]
`LISTEN_PID`, which a conforming client checks against its own PID
before trusting `LISTEN_FDS`, is appended by the child itself after the
clone, since only then is the PID known
[*fdstore.listen-pid-is-appended-by-the-child]; a value for it from
either configurable environment layer is discarded so that it cannot
shadow the real one (§5.5).

A submitted job receives its attached descriptors by exactly this path
(§8.5).

A listening socket that comes back this way still conveys the identity
captured when the previous instance called `listen()` on it. peinit
does not touch that
[*fdstore.a-returned-listener-keeps-its-captured-identity]:
a service that wants connecting clients to see the
instance actually accepting restamps the returned listeners itself
(`peios_socket_restamp`, the Peios Kernel TRM §3.5) before it starts
accepting.

Injection happens for the main process only. Hooks and health checks
never receive stored descriptors.
[*fdstore.only-the-main-process-receives-stored-descriptors]

The store is cleared on a successful injection, at the top of the
started-launch handling. A launch that *fails* does not clear it, so the
descriptors survive a failed attempt and are available to the next one.
[*fdstore.a-failed-launch-does-not-clear-the-store]

## Clearing

The store is cleared, and its descriptors closed, when:

- **The service is stopped explicitly** — by an administrator, or by
  shutdown. The distinction peinit draws is the operation's type and
  source together: an administrator's stop clears, and a
  restart-policy-sourced stop does not.
  [*fdstore.an-explicit-stop-clears-the-store] The service is not coming
  back from an explicit stop, so the descriptors are unneeded.
- **The definition is removed** and its entry finally discarded (§3.8),
  immediately if nothing was running and on the instance's exit
  otherwise. [*fdstore.a-discarded-definition-clears-the-store]

It **survives** an automatic restart — crash, restart policy, new start
— which is the entire point. The descriptors persist through exactly the
restart the service did not choose and cannot prepare for.
[*fdstore.the-store-survives-an-automatic-restart]

> [!NOTE]
> The `LISTEN_FDS` and `LISTEN_FDNAMES` convention is systemd's, so
> software already written to receive descriptors that way works
> unmodified. peinit's own C client library exposes the notification
> helpers but no descriptor-passing helper, so reaching the store from
> that library means constructing the control message by hand.
