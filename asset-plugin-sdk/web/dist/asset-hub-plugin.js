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
}), C = (t) => new e("CONNECTION_DESTROYED", `Method call ${g(t)}() failed due to destroyed connection`), ee = (t, r, a) => {
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
}, w = () => {
	let e, t;
	return {
		promise: new Promise((n, r) => {
			e = n, t = r;
		}),
		resolve: e,
		reject: t
	};
}, T = "deprecated-penpal", E = (e) => a(e) && "penpal" in e, D = (e) => e.split("."), O = (e) => e.join("."), k = (e) => {
	try {
		return JSON.stringify(e);
	} catch {
		return String(e);
	}
}, A = (t) => new e("TRANSMISSION_FAILED", `Unexpected message to translate: ${k(t)}`), j = (e) => {
	if (e.penpal === "syn") return {
		namespace: i,
		channel: void 0,
		type: "SYN",
		participantId: T
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
		methodPath: D(e.methodName),
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
	throw A(e);
}, M = (e) => {
	if (l(e)) return {
		penpal: "synAck",
		methodNames: e.methodPaths.map(O)
	};
	if (d(e)) return {
		penpal: "call",
		id: e.id,
		methodName: O(e.methodPath),
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
	throw A(e);
}, N = ({ messenger: t, methods: n, timeout: r, channel: a, log: o }) => {
	let s = y(), d, f = [], p = !1, h = m(n), { promise: g, resolve: _, reject: b } = w(), x = r === void 0 ? void 0 : setTimeout(() => {
		b(new e("CONNECTION_TIMEOUT", `Connection timed out after ${r}ms`));
	}, r), S = () => {
		for (let e of f) e();
	}, C = () => {
		if (p) return;
		f.push(v(t, n, a, o));
		let { remoteProxy: e, destroy: r } = ee(t, a, o);
		f.push(r), clearTimeout(x), p = !0, _({
			remoteProxy: e,
			destroy: S
		});
	}, E = () => {
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
	}, D = (n) => {
		if (o?.("Received handshake SYN", n), n.participantId === d && d !== T || (d = n.participantId, E(), !(s > d || d === T))) return;
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
	}, O = (n) => {
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
	}, k = (e) => {
		o?.("Received handshake ACK2", e), C();
	}, A = (e) => {
		c(e) && D(e), l(e) && O(e), u(e) && k(e);
	};
	return t.addMessageHandler(A), f.push(() => t.removeMessageHandler(A)), E(), g;
}, P = (e) => {
	let t = !1, n;
	return (...r) => (t || (t = !0, n = e(...r)), n);
}, F = /* @__PURE__ */ new WeakSet(), I = ({ messenger: t, methods: n = {}, timeout: r, channel: a, log: o }) => {
	if (!t) throw new e("INVALID_ARGUMENT", "messenger must be defined");
	if (F.has(t)) throw new e("INVALID_ARGUMENT", "A messenger can only be used for a single connection");
	F.add(t);
	let c = [t.destroy], l = P((e) => {
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
				let { remoteProxy: e, destroy: i } = await N({
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
}, L = class {
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
			let e = this.#s ? M(t) : t, r = this.#l(t);
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
			if (E(r)) {
				this.#n?.("Please upgrade the child window to the latest version of Penpal."), this.#s = !0;
				try {
					r = j(r);
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
}, R = "asset-hub.plugin-api@1", z = "asset-hub.plugin-frame@1", B = "asset-hub.plugin-directory-frame@1", V = ["executeResourceAction", "replaceResourceText"], H = [
	"executeDirectoryAction",
	"viewResource",
	"refreshDirectory",
	"navigateToDirectory",
	"editResource"
], U = [
	"text",
	"markdown",
	"html",
	"plugin_frame",
	"json",
	"media",
	"download"
], W = ["replace_content", "delete"], G = [
	"update",
	"create_child",
	"create_tree",
	"delete"
], K = 1e4, q = 3e4;
async function J(e = {}) {
	if (window.parent === window) throw Error("Asset Hub Plugin Web SDK must run inside a plugin frame.");
	let t = $(e.connectionTimeoutMs, K, "connectionTimeoutMs"), n = $(e.callTimeoutMs, q, "callTimeoutMs"), r = I({
		messenger: new L({
			remoteWindow: window.parent,
			allowedOrigins: ["*"]
		}),
		channel: z,
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
async function Y(e = {}) {
	if (window.parent === window) throw Error("Asset Hub Directory Plugin Web SDK must run inside a plugin frame.");
	let t = $(e.connectionTimeoutMs, K, "connectionTimeoutMs"), n = $(e.callTimeoutMs, q, "callTimeoutMs"), r = I({
		messenger: new L({
			remoteWindow: window.parent,
			allowedOrigins: ["*"]
		}),
		channel: B,
		timeout: t
	}), i = await r.promise;
	return {
		executeDirectoryAction(e, t) {
			return i.executeDirectoryAction(e, t ?? {}, new b({ timeout: n }));
		},
		viewResource(e, t) {
			return i.viewResource(e, t ?? {}, new b({ timeout: n }));
		},
		refreshDirectory() {
			return i.refreshDirectory(new b({ timeout: n }));
		},
		navigateToDirectory(e) {
			return i.navigateToDirectory(e, new b({ timeout: n }));
		},
		editResource(e) {
			return i.editResource(e, new b({ timeout: n }));
		},
		disconnect() {
			r.destroy();
		}
	};
}
function X({ client: e, frame: t, resourceId: n, output: r, connectionTimeoutMs: i }) {
	if (r.resourceId !== n) throw Error("The Resource frame output is not bound to the requested Resource.");
	let a = r.view;
	if (a?.view !== "plugin_frame") throw Error("The Resource Action did not return a plugin_frame.");
	if (a.plugin_api !== "asset-hub.plugin-api@1") throw Error(`Unsupported Plugin Frame API: ${a.plugin_api}`);
	let o = Z(a.url), s = t.contentWindow;
	if (!s) throw Error("The Resource frame window is not available.");
	t.setAttribute("sandbox", "allow-scripts"), t.title = a.title ?? "Resource view", t.src = o;
	let c = $(i, K, "connectionTimeoutMs"), l = I({
		messenger: new L({
			remoteWindow: s,
			allowedOrigins: ["*"]
		}),
		channel: z,
		timeout: c,
		methods: {
			executeResourceAction(t, i) {
				return t === r.action ? e.viewResource(n, i ?? {}) : Promise.reject(/* @__PURE__ */ Error("The nested Resource frame may execute only its originating Action."));
			},
			replaceResourceText() {
				return Promise.reject(/* @__PURE__ */ Error("Text replacement is not available from an embedded read-only Resource frame."));
			}
		}
	});
	return {
		ready: l.promise.then(() => void 0),
		disconnect() {
			l.destroy();
		}
	};
}
function Z(e) {
	let [t] = e.split(/[?#]/, 1);
	if (!t || !/^\/plugins\/[a-z0-9._-]+\//.test(t) || Q(t)) throw Error("The Resource Action returned an invalid plugin frame URL.");
	return e;
}
function Q(e) {
	try {
		return e.split("/").some((e) => {
			let t = decodeURIComponent(e);
			return t === "." || t === ".." || t.includes("/") || t.includes("\\");
		});
	} catch {
		return !0;
	}
}
function $(e, t, n) {
	if (e === void 0) return t;
	if (!Number.isSafeInteger(e) || e <= 0) throw TypeError(`${n} must be a positive safe integer.`);
	return e;
}
//#endregion
export { B as DIRECTORY_FRAME_CHANNEL, R as PLUGIN_API_VERSION, z as RESOURCE_FRAME_CHANNEL, Y as connectAssetHubDirectoryFrame, J as connectAssetHubFrame, G as directoryActionEffectKinds, H as directoryFrameMethods, X as mountAssetHubResourceFrame, U as pluginViewKinds, W as resourceActionEffectKinds, V as resourceFrameMethods };
