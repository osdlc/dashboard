// An experimental macOS screensaver: a WKWebView showing the dashboard's
// ambient page. See docs/desktop.md ("macOS .saver experiment").
//
// The page needs a running server and its session token. The saver takes the
// URL and token from, in order: $DASHBOARD_SAVER_URL and $DASHBOARD_SAVER_TOKEN
// (for testing outside the screensaver host), the `url` and `token`
// screensaver defaults (module dev.dashboard.saver), then the record
// `dashboard serve --detach` writes (~/.local/share/run/dashboard/serve.json).
// The token goes in the page's sessionStorage before the page runs, where the
// page looks for a session it kept earlier; it never goes in the URL. The screensaver host
// (legacyScreenSaver) is sandboxed but may read any file and open network
// connections, so both work in principle.

#import <ScreenSaver/ScreenSaver.h>
#import <WebKit/WebKit.h>
#include <pwd.h>
#include <unistd.h>

@interface DashboardSaverView : ScreenSaverView <WKNavigationDelegate>
@property(strong) WKWebView *web;
@property(strong) NSTextField *message;
@end

@implementation DashboardSaverView

// The real home: in the sandbox, NSHomeDirectory() is the host's container.
static NSString *realHome(void) {
  struct passwd *pw = getpwuid(getuid());
  return pw ? [NSString stringWithUTF8String:pw->pw_dir] : NSHomeDirectory();
}

// The server's page and session token, or nil when no server is known.
+ (NSDictionary *)server {
  NSDictionary *env = NSProcessInfo.processInfo.environment;
  NSString *url = env[@"DASHBOARD_SAVER_URL"];
  NSString *token = env[@"DASHBOARD_SAVER_TOKEN"];
  if (url.length == 0) {
    ScreenSaverDefaults *defaults = [ScreenSaverDefaults defaultsForModuleWithName:@"dev.dashboard.saver"];
    url = [defaults stringForKey:@"url"];
    token = [defaults stringForKey:@"token"];
  }
  if (url.length == 0) {
    NSString *record = [realHome() stringByAppendingPathComponent:@".local/share/run/dashboard/serve.json"];
    NSData *data = [NSData dataWithContentsOfFile:record];
    NSDictionary *json = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
    if ([json isKindOfClass:[NSDictionary class]]) {
      url = json[@"url"];
      token = json[@"token"];
    }
  }
  if (![url isKindOfClass:[NSString class]] || url.length == 0) return nil;
  NSString *page = [url stringByAppendingString:([url containsString:@"?"] ? @"&mode=ambient" : @"?mode=ambient")];
  return @{@"url" : page, @"token" : ([token isKindOfClass:[NSString class]] ? token : @"")};
}

// Sets the session token in sessionStorage, only on the server's own origin.
+ (WKUserScript *)signInScript:(NSURL *)page token:(NSString *)token {
  NSString *origin = [NSString stringWithFormat:@"%@://%@:%@", page.scheme, page.host, page.port];
  NSData *json = [NSJSONSerialization dataWithJSONObject:@[ origin, token ] options:0 error:nil];
  NSString *args = [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding];
  NSString *source = [NSString stringWithFormat:
      @"(([origin, token]) => { if (token && location.origin === origin) try { "
       "sessionStorage.setItem('dashboard-session', token) } catch {} })(%@)", args];
  return [[WKUserScript alloc] initWithSource:source
                                injectionTime:WKUserScriptInjectionTimeAtDocumentStart
                             forMainFrameOnly:YES];
}

- (instancetype)initWithFrame:(NSRect)frame isPreview:(BOOL)isPreview {
  self = [super initWithFrame:frame isPreview:isPreview];
  if (!self) return nil;
  self.animationTimeInterval = 1.0;

  self.message = [NSTextField labelWithString:@"Dashboard: starting…"];
  self.message.textColor = NSColor.secondaryLabelColor;
  self.message.frame = NSInsetRect(self.bounds, 40, 40);
  self.message.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
  [self addSubview:self.message];

  NSDictionary *server = [DashboardSaverView server];
  if (!server) {
    self.message.stringValue = @"Dashboard: no server found. Run `dashboard serve --detach`.";
    return self;
  }
  NSURL *page = [NSURL URLWithString:server[@"url"]];
  WKWebViewConfiguration *config = [WKWebViewConfiguration new];
  [config.userContentController addUserScript:[DashboardSaverView signInScript:page token:server[@"token"]]];
  self.web = [[WKWebView alloc] initWithFrame:self.bounds configuration:config];
  self.web.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
  self.web.navigationDelegate = self;
  self.web.hidden = YES;
  [self addSubview:self.web];
  [self.web loadRequest:[NSURLRequest requestWithURL:page]];
  return self;
}

- (void)webView:(WKWebView *)webView didFinishNavigation:(WKNavigation *)navigation {
  webView.hidden = NO;
  self.message.hidden = YES;
  NSLog(@"DashboardSaver: loaded %@", webView.URL.host);
}

- (void)webView:(WKWebView *)webView didFailProvisionalNavigation:(WKNavigation *)navigation withError:(NSError *)error {
  self.message.stringValue = [NSString stringWithFormat:@"Dashboard: can't reach the server (%@)", error.localizedDescription];
  NSLog(@"DashboardSaver: load failed: %@", error);
}

- (void)drawRect:(NSRect)rect {
  [NSColor.blackColor setFill];
  NSRectFill(rect);
}

- (BOOL)hasConfigureSheet {
  return NO;
}

@end
