//#region src/contract.ts
var e = "asset-hub.plugin-api@1", t = "asset-hub.plugin-frame@1", n = "asset-hub.plugin-directory-frame@1", r = ["executeResourceAction", "replaceResourceText"], i = [
	"executeDirectoryAction",
	"viewResource",
	"refreshDirectory",
	"navigateToDirectory",
	"editResource"
], a = [
	"text",
	"markdown",
	"html",
	"plugin_frame",
	"json",
	"media",
	"download"
], o = ["replace_content", "delete"], s = [
	"update",
	"create_child",
	"create_tree",
	"delete"
];
//#endregion
export { n as DIRECTORY_FRAME_CHANNEL, e as PLUGIN_API_VERSION, t as RESOURCE_FRAME_CHANNEL, s as directoryActionEffectKinds, i as directoryFrameMethods, a as pluginViewKinds, o as resourceActionEffectKinds, r as resourceFrameMethods };
