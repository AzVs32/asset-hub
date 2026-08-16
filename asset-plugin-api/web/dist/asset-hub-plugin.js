var e = class extends Error {
	code;
	constructor(e, t) {
		super(t), this.name = "PenpalError", this.code = e;
	}
}, t = (t) => ({
	name: t.name,
	message: t.message,
	stack: t.stack,
	penpalCode: t instanceof e ? t.code : void 0
}), n = ({ name: t, message: n, stack: r, penpalCode: i }) => {
	let a = i ? new e(i, n) : Error(n);
	return a.name = t, a.stack = r, a;
}, r = class {
	value;
	transferables;
	constructor(e, t) {
		this.value = e, this.transferables = t?.transferables;
	}
}, i = "penpal", a = (e) => typeof e == "object" && !!e, o = (e) => typeof e == "function", s = (e) => a(e) && e.namespace === i, c = (e) => e.type === "SYN", l = (e) => e.type === "ACK1", u = (e) => e.type === "ACK2", d = (e) => e.type === "CALL", f = (e) => e.type === "REPLY", p = (e) => e.type === "DESTROY", m = (e, t = []) => {
	let n = [];
	for (let r of Object.keys(e)) {
		let i = e[r];
		o(i) ? n.push([...t, r]) : a(i) && n.push(...m(i, [...t, r]));
	}
	return n;
}, h = (e, t) => {
	let n = e.reduce((e, t) => a(e) ? e[t] : void 0, t);
	return o(n) ? n : void 0;
}, g = (e) => e.join("."), _ = (e, n, r) => ({
	namespace: i,
	channel: e,
	type: "REPLY",
	callId: n,
	isError: !0,
	...r instanceof Error ? {
		value: t(r),
		isSerializedErrorInstance: !0
	} : { value: r }
}), v = (t, n, a, o) => {
	let s = !1, c = async (c) => {
		if (s || !d(c)) return;
		o?.(`Received ${g(c.methodPath)}() call`, c);
		let { methodPath: l, args: u, id: f } = c, p, m;
		try {
			let t = h(l, n);
			if (!t) throw new e("METHOD_NOT_FOUND", `Method \`${g(l)}\` is not found.`);
			let o = await t(...u);
			o instanceof r && (m = o.transferables, o = await o.value), p = {
				namespace: i,
				channel: a,
				type: "REPLY",
				callId: f,
				value: o
			};
		} catch (e) {
			p = _(a, f, e);
		}
		if (!s) try {
			o?.(`Sending ${g(l)}() reply`, p), t.sendMessage(p, m);
		} catch (e) {
			throw e.name === "DataCloneError" && (p = _(a, f, e), o?.(`Sending ${g(l)}() reply`, p), t.sendMessage(p)), e;
		}
	};
	return t.addMessageHandler(c), () => {
		s = !0, t.removeMessageHandler(c);
	};
}, y = crypto.randomUUID?.bind(crypto) ?? (() => [
	,
	,
	,
	,
].fill(0).map(() => Math.floor(Math.random() * (2 ** 53 - 1)).toString(16)).join("-")), b = class {
	transferables;
	timeout;
	constructor(e) {
		this.transferables = e?.transferables, this.timeout = e?.timeout;
	}
}, x = /* @__PURE__ */ new Set([
	"apply",
	"call",
	"bind"
]), S = (e, t, n = []) => new Proxy(n.length ? () => {} : /* @__PURE__ */ Object.create(null), {
	get(r, i) {
		if (i !== "then") return n.length && x.has(i) ? Reflect.get(r, i) : S(e, t, [...n, i]);
	},
	apply(t, r, i) {
		return e(n, i);
	}
}), C = (t) => new e("CONNECTION_DESTROYED", `Method call ${g(t)}() failed due to destroyed connection`), w = (t, r, a) => {
	let o = !1, s = /* @__PURE__ */ new Map(), c = (e) => {
		if (!f(e)) return;
		let { callId: t, value: r, isError: i, isSerializedErrorInstance: o } = e, c = s.get(t);
		c && (s.delete(t), a?.(`Received ${g(c.methodPath)}() call`, e), i ? c.reject(o ? n(r) : r) : c.resolve(r));
	};
	return t.addMessageHandler(c), {
		remoteProxy: S((n, c) => {
			if (o) throw C(n);
			let l = y(), u = c[c.length - 1], d = u instanceof b, { timeout: f, transferables: p } = d ? u : {}, m = d ? c.slice(0, -1) : c;
			return new Promise((o, c) => {
				let u = f === void 0 ? void 0 : window.setTimeout(() => {
					s.delete(l), c(new e("METHOD_CALL_TIMEOUT", `Method call ${g(n)}() timed out after ${f}ms`));
				}, f);
				s.set(l, {
					methodPath: n,
					resolve: o,
					reject: c,
					timeoutId: u
				});
				try {
					let e = {
						namespace: i,
						channel: r,
						type: "CALL",
						id: l,
						methodPath: n,
						args: m
					};
					a?.(`Sending ${g(n)}() call`, e), t.sendMessage(e, p);
				} catch (t) {
					c(new e("TRANSMISSION_FAILED", t.message));
				}
			});
		}, a),
		destroy: () => {
			o = !0, t.removeMessageHandler(c);
			for (let { methodPath: e, reject: t, timeoutId: n } of s.values()) clearTimeout(n), t(C(e));
			s.clear();
		}
	};
}, T = () => {
	let e, t;
	return {
		promise: new Promise((n, r) => {
			e = n, t = r;
		}),
		resolve: e,
		reject: t
	};
}, E = "deprecated-penpal", D = (e) => a(e) && "penpal" in e, O = (e) => e.split("."), k = (e) => e.join("."), A = (e) => {
	try {
		return JSON.stringify(e);
	} catch {
		return String(e);
	}
}, j = (t) => new e("TRANSMISSION_FAILED", `Unexpected message to translate: ${A(t)}`), M = (e) => {
	if (e.penpal === "syn") return {
		namespace: i,
		channel: void 0,
		type: "SYN",
		participantId: E
	};
	if (e.penpal === "ack") return {
		namespace: i,
		channel: void 0,
		type: "ACK2"
	};
	if (e.penpal === "call") return {
		namespace: i,
		channel: void 0,
		type: "CALL",
		id: e.id,
		methodPath: O(e.methodName),
		args: e.args
	};
	if (e.penpal === "reply") return e.resolution === "fulfilled" ? {
		namespace: i,
		channel: void 0,
		type: "REPLY",
		callId: e.id,
		value: e.returnValue
	} : {
		namespace: i,
		channel: void 0,
		type: "REPLY",
		callId: e.id,
		isError: !0,
		...e.returnValueIsError ? {
			value: e.returnValue,
			isSerializedErrorInstance: !0
		} : { value: e.returnValue }
	};
	throw j(e);
}, N = (e) => {
	if (l(e)) return {
		penpal: "synAck",
		methodNames: e.methodPaths.map(k)
	};
	if (d(e)) return {
		penpal: "call",
		id: e.id,
		methodName: k(e.methodPath),
		args: e.args
	};
	if (f(e)) return e.isError ? {
		penpal: "reply",
		id: e.callId,
		resolution: "rejected",
		...e.isSerializedErrorInstance ? {
			returnValue: e.value,
			returnValueIsError: !0
		} : { returnValue: e.value }
	} : {
		penpal: "reply",
		id: e.callId,
		resolution: "fulfilled",
		returnValue: e.value
	};
	throw j(e);
}, P = ({ messenger: t, methods: n, timeout: r, channel: a, log: o }) => {
	let s = y(), d, f = [], p = !1, h = m(n), { promise: g, resolve: _, reject: b } = T(), x = r === void 0 ? void 0 : setTimeout(() => {
		b(new e("CONNECTION_TIMEOUT", `Connection timed out after ${r}ms`));
	}, r), S = () => {
		for (let e of f) e();
	}, C = () => {
		if (p) return;
		f.push(v(t, n, a, o));
		let { remoteProxy: e, destroy: r } = w(t, a, o);
		f.push(r), clearTimeout(x), p = !0, _({
			remoteProxy: e,
			destroy: S
		});
	}, D = () => {
		let n = {
			namespace: i,
			type: "SYN",
			channel: a,
			participantId: s
		};
		o?.("Sending handshake SYN", n);
		try {
			t.sendMessage(n);
		} catch (t) {
			b(new e("TRANSMISSION_FAILED", t.message));
		}
	}, O = (n) => {
		if (o?.("Received handshake SYN", n), n.participantId === d && d !== E || (d = n.participantId, D(), !(s > d || d === E))) return;
		let r = {
			namespace: i,
			channel: a,
			type: "ACK1",
			methodPaths: h
		};
		o?.("Sending handshake ACK1", r);
		try {
			t.sendMessage(r);
		} catch (t) {
			b(new e("TRANSMISSION_FAILED", t.message));
			return;
		}
	}, k = (n) => {
		o?.("Received handshake ACK1", n);
		let r = {
			namespace: i,
			channel: a,
			type: "ACK2"
		};
		o?.("Sending handshake ACK2", r);
		try {
			t.sendMessage(r);
		} catch (t) {
			b(new e("TRANSMISSION_FAILED", t.message));
			return;
		}
		C();
	}, A = (e) => {
		o?.("Received handshake ACK2", e), C();
	}, j = (e) => {
		c(e) && O(e), l(e) && k(e), u(e) && A(e);
	};
	return t.addMessageHandler(j), f.push(() => t.removeMessageHandler(j)), D(), g;
}, F = (e) => {
	let t = !1, n;
	return (...r) => (t || (t = !0, n = e(...r)), n);
}, I = /* @__PURE__ */ new WeakSet(), L = ({ messenger: t, methods: n = {}, timeout: r, channel: a, log: o }) => {
	if (!t) throw new e("INVALID_ARGUMENT", "messenger must be defined");
	if (I.has(t)) throw new e("INVALID_ARGUMENT", "A messenger can only be used for a single connection");
	I.add(t);
	let c = [t.destroy], l = F((e) => {
		if (e) {
			let e = {
				namespace: i,
				channel: a,
				type: "DESTROY"
			};
			try {
				t.sendMessage(e);
			} catch {}
		}
		for (let e of c) e();
		o?.("Connection destroyed");
	}), u = (e) => s(e) && e.channel === a;
	return {
		promise: (async () => {
			try {
				t.initialize({
					log: o,
					validateReceivedMessage: u
				}), t.addMessageHandler((e) => {
					p(e) && l(!1);
				});
				let { remoteProxy: e, destroy: i } = await P({
					messenger: t,
					methods: n,
					timeout: r,
					channel: a,
					log: o
				});
				return c.push(i), e;
			} catch (e) {
				throw l(!0), e;
			}
		})(),
		destroy: () => {
			l(!0);
		}
	};
}, R = class {
	#e;
	#t;
	#n;
	#r;
	#i;
	#a = /* @__PURE__ */ new Set();
	#o;
	#s = !1;
	constructor({ remoteWindow: t, allowedOrigins: n }) {
		if (!t) throw new e("INVALID_ARGUMENT", "remoteWindow must be defined");
		this.#e = t, this.#t = n?.length ? n : [window.origin];
	}
	initialize = ({ log: e, validateReceivedMessage: t }) => {
		this.#n = e, this.#r = t, window.addEventListener("message", this.#d);
	};
	sendMessage = (t, n) => {
		if (c(t)) {
			let e = this.#l(t);
			this.#e.postMessage(t, {
				targetOrigin: e,
				transfer: n
			});
			return;
		}
		if (l(t) || this.#s) {
			let e = this.#s ? N(t) : t, r = this.#l(t);
			this.#e.postMessage(e, {
				targetOrigin: r,
				transfer: n
			});
			return;
		}
		if (u(t)) {
			let { port1: e, port2: r } = new MessageChannel();
			this.#o = e, e.addEventListener("message", this.#f), e.start();
			let i = [r, ...n || []], a = this.#l(t);
			this.#e.postMessage(t, {
				targetOrigin: a,
				transfer: i
			});
			return;
		}
		if (this.#o) {
			this.#o.postMessage(t, { transfer: n });
			return;
		}
		throw new e("TRANSMISSION_FAILED", "Cannot send message because the MessagePort is not connected");
	};
	addMessageHandler = (e) => {
		this.#a.add(e);
	};
	removeMessageHandler = (e) => {
		this.#a.delete(e);
	};
	destroy = () => {
		window.removeEventListener("message", this.#d), this.#u(), this.#a.clear();
	};
	#c = (e) => this.#t.some((t) => t instanceof RegExp ? t.test(e) : t === e || t === "*");
	#l = (t) => {
		if (c(t)) return "*";
		if (!this.#i) throw new e("TRANSMISSION_FAILED", "Cannot send message because the remote origin is not established");
		return this.#i === "null" && this.#t.includes("*") ? "*" : this.#i;
	};
	#u = () => {
		this.#o?.removeEventListener("message", this.#f), this.#o?.close(), this.#o = void 0;
	};
	#d = ({ source: e, origin: t, ports: n, data: r }) => {
		if (e === this.#e) {
			if (D(r)) {
				this.#n?.("Please upgrade the child window to the latest version of Penpal."), this.#s = !0;
				try {
					r = M(r);
				} catch (e) {
					this.#n?.(`Failed to translate deprecated message: ${e.message}`);
					return;
				}
			}
			if (this.#r?.(r)) {
				if (!this.#c(t)) {
					this.#n?.(`Received a message from origin \`${t}\` which did not match allowed origins \`[${this.#t.join(", ")}]\``);
					return;
				}
				if (c(r) && (this.#u(), this.#i = t), u(r) && !this.#s) {
					if (this.#o = n[0], !this.#o) {
						this.#n?.("Ignoring ACK2 because it did not include a MessagePort");
						return;
					}
					this.#o.addEventListener("message", this.#f), this.#o.start();
				}
				for (let e of this.#a) e(r);
			}
		}
	};
	#f = ({ data: e }) => {
		if (this.#r?.(e)) for (let t of this.#a) t(e);
	};
}, z = "asset-hub.plugin-api@5", B = "asset-hub.plugin-frame@5", V = "asset-hub.plugin-directory-frame@5", H = [
	"text",
	"markdown",
	"html",
	"plugin_frame",
	"json",
	"media",
	"download"
], U = ["replace_content", "delete"], W = [
	"update",
	"create_child",
	"create_tree",
	"delete"
], G = 1e4, K = 3e4;
async function q(e = {}) {
	if (window.parent === window) throw Error("Asset Hub Plugin Web SDK must run inside a plugin frame.");
	let t = Y(e.connectionTimeoutMs, G, "connectionTimeoutMs"), n = Y(e.callTimeoutMs, K, "callTimeoutMs"), r = L({
		messenger: new R({
			remoteWindow: window.parent,
			allowedOrigins: ["*"]
		}),
		channel: B,
		timeout: t
	}), i = await r.promise;
	return {
		executeResourceAction(e, t) {
			let r = new b({ timeout: n });
			return i.executeResourceAction(e, t ?? {}, r);
		},
		replaceResourceText(e) {
			return i.replaceResourceText(e, new b({ timeout: n }));
		},
		disconnect() {
			r.destroy();
		}
	};
}
async function J(e = {}) {
	if (window.parent === window) throw Error("Asset Hub Directory Plugin Web SDK must run inside a plugin frame.");
	let t = Y(e.connectionTimeoutMs, G, "connectionTimeoutMs"), n = Y(e.callTimeoutMs, K, "callTimeoutMs"), r = L({
		messenger: new R({
			remoteWindow: window.parent,
			allowedOrigins: ["*"]
		}),
		channel: V,
		timeout: t
	}), i = await r.promise;
	return {
		executeDirectoryAction(e, t) {
			return i.executeDirectoryAction(e, t ?? {}, new b({ timeout: n }));
		},
		refreshDirectory() {
			return i.refreshDirectory(new b({ timeout: n }));
		},
		navigateToDirectory(e) {
			return i.navigateToDirectory(e, new b({ timeout: n }));
		},
		disconnect() {
			r.destroy();
		}
	};
}
function Y(e, t, n) {
	if (e === void 0) return t;
	if (!Number.isSafeInteger(e) || e <= 0) throw TypeError(`${n} must be a positive safe integer.`);
	return e;
}
//#endregion
export { V as DIRECTORY_FRAME_CHANNEL, z as PLUGIN_API_VERSION, B as RESOURCE_FRAME_CHANNEL, J as connectAssetHubDirectoryFrame, q as connectAssetHubFrame, W as directoryActionEffectKinds, H as pluginViewKinds, U as resourceActionEffectKinds };
