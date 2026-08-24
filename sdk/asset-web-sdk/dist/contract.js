//#region src/contract.ts
var e = "asset-hub.plugin-api@1", t = "asset-hub.plugin-frame@1", n = "asset-hub.plugin-directory-frame@1", r = [
	"thumbnail",
	"view",
	"edit"
], i = r[0], a = r[1], o = r[2], s = ["thumbnail", "workspace"], c = s[0], l = s[1], u = ["executeResourceAction", "replaceResourceText"], d = [
	"executeDirectoryAction",
	"viewResource",
	"refreshDirectory",
	"navigateToDirectory",
	"editResource"
], f = [
	"text",
	"markdown",
	"html",
	"plugin_frame",
	"json",
	"media",
	"download"
], p = ["replace_content", "delete"], m = [
	"update",
	"create_child",
	"create_tree",
	"delete"
];
//#endregion
export { n as DIRECTORY_FRAME_CHANNEL, c as DIRECTORY_THUMBNAIL_CAPABILITY, l as DIRECTORY_WORKSPACE_CAPABILITY, e as PLUGIN_API_VERSION, o as RESOURCE_EDIT_CAPABILITY, t as RESOURCE_FRAME_CHANNEL, i as RESOURCE_THUMBNAIL_CAPABILITY, a as RESOURCE_VIEW_CAPABILITY, s as directoryActionCapabilityIds, m as directoryActionEffectKinds, d as directoryFrameMethods, f as pluginViewKinds, r as resourceActionCapabilityIds, p as resourceActionEffectKinds, u as resourceFrameMethods };
