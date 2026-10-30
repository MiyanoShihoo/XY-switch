import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
const root = resolve(process.argv[2]);
assert(root);
const project = join(root, 'NativeAcceptance.xcodeproj');
await mkdir(join(project, 'xcshareddata', 'xcschemes'), { recursive: true });
const id = number => number.toString(16).toUpperCase().padStart(24, '0');
const objects = [];
function obj(number, body) { objects.push(`${id(number)} = { ${body} };`); }
obj(1, `isa = PBXProject; attributes = { LastUpgradeCheck = 1600; TargetAttributes = { ${id(10)} = { CreatedOnToolsVersion = 16.0; }; ${id(11)} = { CreatedOnToolsVersion = 16.0; TestTargetID = ${id(10)}; }; }; }; buildConfigurationList = ${id(20)}; compatibilityVersion = "Xcode 14.0"; developmentRegion = en; knownRegions = (en, Base); mainGroup = ${id(2)}; productRefGroup = ${id(3)}; projectDirPath = ""; projectRoot = ""; targets = (${id(10)}, ${id(11)});`);
obj(2, `isa = PBXGroup; children = (${id(4)}, ${id(5)}, ${id(3)}); sourceTree = "<group>";`);
obj(3, `isa = PBXGroup; children = (${id(6)}, ${id(7)}); name = Products; sourceTree = "<group>";`);
for (const [n, path, type] of [[4, 'Host.swift', 'sourcecode.swift'], [5, 'NativeAcceptanceTests.swift', 'sourcecode.swift']]) obj(n, `isa = PBXFileReference; lastKnownFileType = ${type}; path = ${path}; sourceTree = "<group>";`);
obj(6, 'isa = PBXFileReference; explicitFileType = wrapper.application; path = NativeAcceptanceHost.app; sourceTree = BUILT_PRODUCTS_DIR;');
obj(7, 'isa = PBXFileReference; explicitFileType = wrapper.cfbundle; path = NativeAcceptanceTests.xctest; sourceTree = BUILT_PRODUCTS_DIR;');
obj(8, `isa = PBXBuildFile; fileRef = ${id(4)};`); obj(9, `isa = PBXBuildFile; fileRef = ${id(5)};`);
obj(10, `isa = PBXNativeTarget; buildConfigurationList = ${id(21)}; buildPhases = (${id(12)}, ${id(14)}, ${id(16)}); buildRules = (); dependencies = (); name = NativeAcceptanceHost; productName = NativeAcceptanceHost; productReference = ${id(6)}; productType = "com.apple.product-type.application";`);
obj(11, `isa = PBXNativeTarget; buildConfigurationList = ${id(22)}; buildPhases = (${id(13)}, ${id(15)}, ${id(17)}); buildRules = (); dependencies = (${id(19)}); name = NativeAcceptanceTests; productName = NativeAcceptanceTests; productReference = ${id(7)}; productType = "com.apple.product-type.bundle.ui-testing";`);
for (const [n, file] of [[12, 8], [13, 9]]) obj(n, `isa = PBXSourcesBuildPhase; buildActionMask = 2147483647; files = (${id(file)}); runOnlyForDeploymentPostprocessing = 0;`);
for (const [n, type] of [[14, 'PBXFrameworksBuildPhase'], [15, 'PBXFrameworksBuildPhase'], [16, 'PBXResourcesBuildPhase'], [17, 'PBXResourcesBuildPhase']]) obj(n, `isa = ${type}; buildActionMask = 2147483647; files = (); runOnlyForDeploymentPostprocessing = 0;`);
obj(18, `isa = PBXContainerItemProxy; containerPortal = ${id(1)}; proxyType = 1; remoteGlobalIDString = ${id(10)}; remoteInfo = NativeAcceptanceHost;`);
obj(19, `isa = PBXTargetDependency; target = ${id(10)}; targetProxy = ${id(18)};`);
for (const [list, config] of [[20, 23], [21, 24], [22, 25]]) obj(list, `isa = XCConfigurationList; buildConfigurations = (${id(config)}); defaultConfigurationIsVisible = 0; defaultConfigurationName = Debug;`);
const shared = 'CLANG_ENABLE_MODULES = YES; CODE_SIGNING_ALLOWED = NO; IPHONEOS_DEPLOYMENT_TARGET = 16.0; SDKROOT = iphoneos; SWIFT_VERSION = 5.0; TARGETED_DEVICE_FAMILY = "1,2";';
obj(23, `isa = XCBuildConfiguration; buildSettings = { ${shared} }; name = Debug;`);
obj(24, 'isa = XCBuildConfiguration; buildSettings = { GENERATE_INFOPLIST_FILE = YES; INFOPLIST_KEY_UILaunchScreen_Generation = YES; PRODUCT_BUNDLE_IDENTIFIER = com.xyswitch.acceptance.host; PRODUCT_NAME = "$(TARGET_NAME)"; SWIFT_EMIT_LOC_STRINGS = NO; }; name = Debug;');
obj(25, 'isa = XCBuildConfiguration; buildSettings = { GENERATE_INFOPLIST_FILE = YES; PRODUCT_BUNDLE_IDENTIFIER = com.xyswitch.acceptance.tests; PRODUCT_NAME = "$(TARGET_NAME)"; TEST_TARGET_NAME = NativeAcceptanceHost; LD_RUNPATH_SEARCH_PATHS = "$(inherited) @executable_path/Frameworks @loader_path/Frameworks"; }; name = Debug;');
await writeFile(join(project, 'project.pbxproj'), `// !$*UTF8*$!\n{ archiveVersion = 1; classes = {}; objectVersion = 56; objects = {\n${objects.join('\n')}\n}; rootObject = ${id(1)}; }\n`);
const reference = (n, name, product) => `<BuildableReference BuildableIdentifier="primary" BlueprintIdentifier="${id(n)}" BuildableName="${product}" BlueprintName="${name}" ReferencedContainer="container:NativeAcceptance.xcodeproj"/>`;
const host = reference(10, 'NativeAcceptanceHost', 'NativeAcceptanceHost.app');
const tests = reference(11, 'NativeAcceptanceTests', 'NativeAcceptanceTests.xctest');
await writeFile(join(project, 'xcshareddata/xcschemes/NativeAcceptance.xcscheme'), `<?xml version="1.0" encoding="UTF-8"?>
<Scheme LastUpgradeVersion="1600" version="1.3"><BuildAction parallelizeBuildables="NO" buildImplicitDependencies="YES"><BuildActionEntries><BuildActionEntry buildForTesting="YES" buildForRunning="YES" buildForProfiling="NO" buildForArchiving="NO" buildForAnalyzing="YES">${host}</BuildActionEntry><BuildActionEntry buildForTesting="YES" buildForRunning="NO" buildForProfiling="NO" buildForArchiving="NO" buildForAnalyzing="YES">${tests}</BuildActionEntry></BuildActionEntries></BuildAction><TestAction buildConfiguration="Debug" selectedDebuggerIdentifier="Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier="Xcode.IDEFoundation.Launcher.LLDB" shouldUseLaunchSchemeArgsEnv="YES"><Testables><TestableReference skipped="NO">${tests}</TestableReference></Testables><MacroExpansion>${host}</MacroExpansion></TestAction><LaunchAction buildConfiguration="Debug" selectedDebuggerIdentifier="Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier="Xcode.IDEFoundation.Launcher.LLDB" launchStyle="0" useCustomWorkingDirectory="NO" ignoresPersistentStateOnLaunch="NO" debugDocumentVersioning="YES" debugServiceExtension="internal" allowLocationSimulation="YES"><BuildableProductRunnable runnableDebuggingMode="0">${host}</BuildableProductRunnable></LaunchAction><ProfileAction buildConfiguration="Debug" shouldUseLaunchSchemeArgsEnv="YES" savedToolIdentifier="" useCustomWorkingDirectory="NO" debugDocumentVersioning="YES"><BuildableProductRunnable runnableDebuggingMode="0">${host}</BuildableProductRunnable></ProfileAction><AnalyzeAction buildConfiguration="Debug"/><ArchiveAction buildConfiguration="Debug" revealArchiveInOrganizer="YES"/></Scheme>\n`);
await writeFile(join(root, 'Host.swift'), `import UIKit
@main final class AcceptanceHost: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
    func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        window = UIWindow(frame: UIScreen.main.bounds)
        window?.rootViewController = UIViewController()
        window?.makeKeyAndVisible()
        return true
    }
}\n`);
await writeFile(join(root, 'NativeAcceptanceTests.swift'), await readFile(new URL('./NativeAcceptanceTests.swift', import.meta.url)));
console.log(project);
