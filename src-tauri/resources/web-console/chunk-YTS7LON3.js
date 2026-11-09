import{l as xt,m as It}from"./chunk-VSLUPSO6.js";import{a as Tt,b as Ct}from"./chunk-I6HLYEP7.js";import{p as yt}from"./chunk-23DMX32T.js";import{$ as j,Ga as K,Ha as E,Ka as gt,Ma as vt,N as ht,Oa as U,S as _t,T as S,U as ft,V as f,Xa as wt,c as pt,da as k,e as M,f as ut,g as z,j as q,k as D,wa as mt}from"./chunk-LRDS3JPN.js";import{$ as V,Ab as m,Bb as w,Cb as ot,Db as st,Eb as H,Fb as x,Jb as I,Kb as r,Lb as Q,Mb as W,Pb as h,Qb as C,Rb as b,Sb as d,Ta as p,U as J,V as $,W as X,Wb as rt,Yb as lt,_ as Y,cc as L,db as B,eb as Z,ec as ct,fa as v,fc as bt,ga as T,hb as N,ja as O,jb as l,k as R,l as G,pa as A,pb as u,qb as c,sb as tt,tb as et,ub as P,vb as g,wb as it,wc as y,xb as nt,xc as dt,yb as at,zb as _}from"./chunk-IJGG6W7R.js";var St=["content"],Ft=["header"],Rt=["lefticon"],Bt=["righticon"],Nt=["closeicon"],kt=["*"];function Pt(e,o){e&1&&H(0)}function Ht(e,o){if(e&1&&(ot(0),l(1,Pt,1,0,"ng-container",3),st()),e&2){let t=r(2);p(),c("ngTemplateOutlet",t.contentTemplate||t._contentTemplate)}}function Qt(e,o){if(e&1&&(_(0,"div",1),W(1),l(2,Ht,2,1,"ng-container",2),m()),e&2){let t=r();c("hidden",!t.selected),u("id",t.tabView.getTabContentId(t.id))("aria-hidden",!t.selected)("aria-labelledby",t.tabView.getTabHeaderActionId(t.id))("data-pc-name","tabpanel"),p(2),c("ngIf",(t.contentTemplate||t._contentTemplate)&&(t.cache?t.loaded:t.selected))}}var Wt=["previousicon"],Lt=["nexticon"],Mt=["navbar"],zt=["prevBtn"],qt=["nextBtn"],jt=["inkbar"],Kt=["elementToObserve"],Ut=e=>({"p-tablist-viewport":e}),Gt=(e,o)=>({"p-tab":!0,"p-tab-active":e,"p-disabled":o});function Jt(e,o){e&1&&w(0,"ChevronLeftIcon"),e&2&&u("aria-hidden",!0)}function Xt(e,o){}function Yt(e,o){e&1&&l(0,Xt,0,0,"ng-template")}function Zt(e,o){if(e&1){let t=x();_(0,"button",12,3),I("click",function(){v(t);let n=r();return T(n.navBackward())}),l(2,Jt,1,1,"ChevronLeftIcon",13)(3,Yt,1,0,null,14),m()}if(e&2){let t=r();u("tabindex",t.tabindex)("aria-label",t.prevButtonAriaLabel),p(2),c("ngIf",!t.previousIconTemplate&&!t._previousIconTemplate),p(),c("ngTemplateOutlet",t.previousIconTemplate&&t._previousIconTemplate)}}function te(e,o){e&1&&H(0)}function ee(e,o){if(e&1&&l(0,te,1,0,"ng-container",14),e&2){let t=r(2).$implicit;c("ngTemplateOutlet",t.headerTemplate||t._headerTemplate)}}function ie(e,o){}function ne(e,o){e&1&&l(0,ie,0,0,"ng-template")}function ae(e,o){if(e&1&&l(0,ne,1,0,null,14),e&2){let t=r(3).$implicit;c("ngTemplateOutlet",t.leftIconTemplate||t._leftIconTemplate)}}function oe(e,o){if(e&1&&w(0,"span",17),e&2){let t=r(3).$implicit;c("ngClass",t.leftIcon)}}function se(e,o){}function re(e,o){e&1&&l(0,se,0,0,"ng-template")}function le(e,o){if(e&1&&l(0,re,1,0,null,14),e&2){let t=r(3).$implicit;c("ngTemplateOutlet",t.rightIconTemplate||t._rightIconTemplate)}}function ce(e,o){if(e&1&&w(0,"span",18),e&2){let t=r(3).$implicit;c("ngClass",t.rightIcon)}}function be(e,o){}function de(e,o){e&1&&l(0,be,0,0,"ng-template")}function pe(e,o){if(e&1&&l(0,de,1,0,null,14),e&2){let t=r(4).$implicit;c("ngTemplateOutlet",t.closeIconTemplate||t._closeIconTemplate)}}function ue(e,o){if(e&1){let t=x();_(0,"TimesIcon",19),I("click",function(n){v(t);let a=r(4).$implicit,s=r();return T(s.close(n,a))}),m()}}function he(e,o){if(e&1&&l(0,pe,1,1)(1,ue,1,0,"TimesIcon"),e&2){let t=r(3).$implicit;g(t.closeIconTemplate||t._closeIconTemplate?0:1)}}function _e(e,o){if(e&1&&(l(0,ae,1,1)(1,oe,1,1,"span",17),rt(2),l(3,le,1,1)(4,ce,1,1,"span",18)(5,he,2,1)),e&2){let t=r(2).$implicit;g(t.leftIconTemplate||t._leftIconTemplate?0:t.leftIcon&&!t.leftIconTemplate&&!t._leftIconTemplate?1:-1),p(2),lt(" ",t.header," "),p(),g(t.rightIconTemplate||t._rightIconTemplate?3:t.rightIcon&&!t.rightIconTemplate&&!t._rightIconTemplate?4:-1),p(2),g(t.closable?5:-1)}}function fe(e,o){if(e&1){let t=x();_(0,"button",15),I("click",function(n){v(t);let a=r().$implicit,s=r();return T(s.open(n,a))})("keydown",function(n){v(t);let a=r().$implicit,s=r();return T(s.onTabKeyDown(n,a))}),l(1,ee,1,1,"ng-container")(2,_e,6,4),m(),w(3,"span",16,4)}if(e&2){let t=r(),i=t.$implicit,n=t.$index,a=r();P(i.headerStyleClass),c("ngClass",bt(22,Gt,i.selected,i.disabled))("ngStyle",i.headerStyle)("pTooltip",i.tooltip)("tooltipPosition",i.tooltipPosition)("positionStyle",i.tooltipPositionStyle)("tooltipStyleClass",i.tooltipStyleClass)("disabled",i.disabled),u("role","tab")("id",a.getTabHeaderActionId(i.id))("aria-controls",a.getTabContentId(i.id))("aria-selected",i.selected)("tabindex",i.disabled||!i.selected?"-1":a.tabindex)("aria-disabled",i.disabled)("data-pc-index",n)("data-p-disabled",i.disabled)("data-pc-section","headeraction")("data-p-active",i.selected),p(),g(i.headerTemplate||i._headerTemplate?1:2),p(2),u("aria-hidden",!0)("data-pc-section","inkbar")}}function me(e,o){if(e&1&&l(0,fe,5,25),e&2){let t=o.$implicit;g(t.closed?-1:0)}}function ge(e,o){}function ve(e,o){e&1&&l(0,ge,0,0,"ng-template")}function Te(e,o){if(e&1&&l(0,ve,1,0,null,14),e&2){let t=r(2);c("ngTemplateOutlet",t.nextIconTemplate||t._nextIconTemplate)}}function Ce(e,o){e&1&&w(0,"ChevronRightIcon"),e&2&&u("aria-hidden",!0)}function ye(e,o){if(e&1){let t=x();_(0,"button",20,5),I("click",function(){v(t);let n=r();return T(n.navForward())}),l(2,Te,1,1)(3,Ce,1,1,"ChevronRightIcon"),m()}if(e&2){let t=r();u("tabindex",t.tabindex)("aria-label",t.nextButtonAriaLabel),p(2),g(t.nextIconTemplate||t._nextIconTemplate?2:3)}}var we=({dt:e})=>`
.p-tabs {
    display: flex;
    flex-direction: column;
}

.p-tablist {
    display: flex;
    position: relative;
}

.p-tabs-scrollable > .p-tablist {
    overflow: hidden;
}

.p-tablist-viewport {
    overflow-x: auto;
    overflow-y: hidden;
    scroll-behavior: smooth;
    scrollbar-width: none;
    overscroll-behavior: contain auto;
}

.p-tablist-viewport::-webkit-scrollbar {
    display: none;
}

.p-tablist-tab-list {
    position: relative;
    display: flex;
    background: ${e("tabs.tablist.background")};
    border-style: solid;
    border-color: ${e("tabs.tablist.border.color")};
    border-width: ${e("tabs.tablist.border.width")};
}

.p-tablist-content {
    flex-grow: 1;
}

.p-tablist-nav-button {
    all: unset;
    position: absolute !important;
    flex-shrink: 0;
    top: 0;
    z-index: 2;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: ${e("tabs.nav.button.background")};
    color: ${e("tabs.nav.button.color")};
    width: ${e("tabs.nav.button.width")};
    transition: color ${e("tabs.transition.duration")}, outline-color ${e("tabs.transition.duration")}, box-shadow ${e("tabs.transition.duration")};
    box-shadow: ${e("tabs.nav.button.shadow")};
    outline-color: transparent;
    cursor: pointer;
}

.p-tablist-nav-button:focus-visible {
    z-index: 1;
    box-shadow: ${e("tabs.nav.button.focus.ring.shadow")};
    outline: ${e("tabs.nav.button.focus.ring.width")} ${e("tabs.nav.button.focus.ring.style")} ${e("tabs.nav.button.focus.ring.color")};
    outline-offset: ${e("tabs.nav.button.focus.ring.offset")};
}

.p-tablist-nav-button:hover {
    color: ${e("tabs.nav.button.hover.color")};
}

.p-tablist-prev-button {
    left: 0;
}

.p-tablist-next-button {
    right: 0;
}

.p-tab {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    cursor: pointer;
    user-select: none;
    position: relative;
    border-style: solid;
    white-space: nowrap;
    gap: ${e("tabs.tab.gap")};
    background: ${e("tabs.tab.background")};
    border-width: ${e("tabs.tab.border.width")};
    border-color: ${e("tabs.tab.border.color")};
    color: ${e("tabs.tab.color")};
    padding: ${e("tabs.tab.padding")};
    font-weight: ${e("tabs.tab.font.weight")};
    transition: background ${e("tabs.transition.duration")}, border-color ${e("tabs.transition.duration")}, color ${e("tabs.transition.duration")}, outline-color ${e("tabs.transition.duration")}, box-shadow ${e("tabs.transition.duration")};
    margin: ${e("tabs.tab.margin")};
    outline-color: transparent;
}

.p-tab:not(.p-disabled):focus-visible {
    z-index: 1;
    box-shadow: ${e("tabs.tab.focus.ring.shadow")};
    outline: ${e("tabs.tab.focus.ring.width")} ${e("tabs.tab.focus.ring.style")} ${e("tabs.tab.focus.ring.color")};
    outline-offset: ${e("tabs.tab.focus.ring.offset")};
}

.p-tab:not(.p-tab-active):not(.p-disabled):hover {
    background: ${e("tabs.tab.hover.background")};
    border-color: ${e("tabs.tab.hover.border.color")};
    color: ${e("tabs.tab.hover.color")};
}

.p-tab-active {
    background: ${e("tabs.tab.active.background")};
    border-color: ${e("tabs.tab.active.border.color")};
    color: ${e("tabs.tab.active.color")};
}

.p-tabpanels {
    background: ${e("tabs.tabpanel.background")};
    color: ${e("tabs.tabpanel.color")};
    padding: ${e("tabs.tabpanel.padding")};
    outline: 0 none;
}

.p-tabpanel:focus-visible {
    box-shadow: ${e("tabs.tabpanel.focus.ring.shadow")};
    outline: ${e("tabs.tabpanel.focus.ring.width")} ${e("tabs.tabpanel.focus.ring.style")} ${e("tabs.tabpanel.focus.ring.color")};
    outline-offset: ${e("tabs.tabpanel.focus.ring.offset")};
}

.p-tablist-active-bar {
    z-index: 1;
    display: block;
    position: absolute;
    bottom: ${e("tabs.active.bar.bottom")};
    height: ${e("tabs.active.bar.height")};
    background: ${e("tabs.active.bar.background")};
    transition: 250ms cubic-bezier(0.35, 0, 0.25, 1);
}
`,xe={root:({props:e})=>["p-tabs p-component",{"p-tabs-scrollable":e.scrollable}]},F=(()=>{class e extends gt{name="tabs";theme=we;classes=xe;static \u0275fac=(()=>{let t;return function(n){return(t||(t=O(e)))(n||e)}})();static \u0275prov=$({token:e,factory:e.\u0275fac})}return e})();var Et=(()=>{class e extends U{closable=!1;get headerStyle(){return this._headerStyle}set headerStyle(t){this._headerStyle=t,this.tabView.cd.markForCheck()}get headerStyleClass(){return this._headerStyleClass}set headerStyleClass(t){this._headerStyleClass=t,this.tabView.cd.markForCheck()}cache=!0;tooltip;tooltipPosition="top";tooltipPositionStyle="absolute";tooltipStyleClass;get selected(){return!!this._selected}set selected(t){this._selected=t,this.loaded||this.cd.detectChanges(),t&&(this.loaded=!0)}get disabled(){return!!this._disabled}set disabled(t){this._disabled=t,this.tabView.cd.markForCheck()}get header(){return this._header}set header(t){this._header=t,Promise.resolve().then(()=>{this.tabView.updateInkBar(),this.tabView.cd.markForCheck()})}get leftIcon(){return this._leftIcon}set leftIcon(t){this._leftIcon=t,this.tabView.cd.markForCheck()}get rightIcon(){return this._rightIcon}set rightIcon(t){this._rightIcon=t,this.tabView.cd.markForCheck()}closed=!1;_headerStyle;_headerStyleClass;_selected;_disabled;_header;_leftIcon;_rightIcon=void 0;loaded=!1;id=mt("pn_id_");contentTemplate;headerTemplate;leftIconTemplate;rightIconTemplate;closeIconTemplate;templates;tabView=V(J(()=>$t));_componentStyle=V(F);_headerTemplate;_contentTemplate;_rightIconTemplate;_leftIconTemplate;_closeIconTemplate;ngAfterContentInit(){this.templates.forEach(t=>{switch(t.getType()){case"header":this._headerTemplate=t.template;break;case"content":this._contentTemplate=t.template;break;case"righticon":this._rightIconTemplate=t.template;break;case"lefticon":this._leftIconTemplate=t.template;break;case"closeicon":this._closeIconTemplate=t.template;break;default:this._contentTemplate=t.template;break}})}static \u0275fac=(()=>{let t;return function(n){return(t||(t=O(e)))(n||e)}})();static \u0275cmp=B({type:e,selectors:[["p-tabPanel"],["p-tabpanel"]],contentQueries:function(i,n,a){if(i&1&&(h(a,St,5),h(a,Ft,5),h(a,Rt,5),h(a,Bt,5),h(a,Nt,5),h(a,K,4)),i&2){let s;b(s=d())&&(n.contentTemplate=s.first),b(s=d())&&(n.headerTemplate=s.first),b(s=d())&&(n.leftIconTemplate=s.first),b(s=d())&&(n.rightIconTemplate=s.first),b(s=d())&&(n.closeIconTemplate=s.first),b(s=d())&&(n.templates=s)}},inputs:{closable:[2,"closable","closable",y],headerStyle:"headerStyle",headerStyleClass:"headerStyleClass",cache:[2,"cache","cache",y],tooltip:"tooltip",tooltipPosition:"tooltipPosition",tooltipPositionStyle:"tooltipPositionStyle",tooltipStyleClass:"tooltipStyleClass",selected:"selected",disabled:"disabled",header:"header",leftIcon:"leftIcon",rightIcon:"rightIcon"},features:[L([F]),N],ngContentSelectors:kt,decls:1,vars:1,consts:[["class","p-tabview-panel","role","tabpanel",3,"hidden",4,"ngIf"],["role","tabpanel",1,"p-tabview-panel",3,"hidden"],[4,"ngIf"],[4,"ngTemplateOutlet"]],template:function(i,n){i&1&&(Q(),l(0,Qt,3,6,"div",0)),i&2&&c("ngIf",!n.closed)},dependencies:[q,M,z,E],encapsulation:2})}return e})(),$t=(()=>{class e extends U{get hostClass(){return this.styleClass}get hostStyle(){return this.style}style;styleClass;controlClose;scrollable;get activeIndex(){return this._activeIndex}set activeIndex(t){if(this._activeIndex=t,this.preventActiveIndexPropagation){this.preventActiveIndexPropagation=!1;return}this.tabs&&this.tabs.length&&this._activeIndex!=null&&this.tabs.length>this._activeIndex&&(this.findSelectedTab().selected=!1,this.tabs[this._activeIndex].selected=!0,this.tabChanged=!0,this.updateScrollBar(t))}selectOnFocus=!1;nextButtonAriaLabel;prevButtonAriaLabel;autoHideButtons=!0;tabindex=0;onChange=new A;onClose=new A;activeIndexChange=new A;content;navbar;prevBtn;nextBtn;inkbar;tabPanels;initialized;tabs;_activeIndex;preventActiveIndexPropagation;tabChanged;backwardIsDisabled=!0;forwardIsDisabled=!1;tabChangesSubscription;resizeObserver;container;list;buttonVisible;elementToObserve;previousIconTemplate;nextIconTemplate;_previousIconTemplate;_nextIconTemplate;_componentStyle=V(F);templates;ngOnInit(){super.ngOnInit(),console.log("TabView component is deprecated as of v18. Use Tabs component instead.")}ngAfterContentInit(){this.initTabs(),this.tabChangesSubscription=this.tabPanels.changes.subscribe(t=>{this.initTabs(),this.refreshButtonState()}),this.templates.forEach(t=>{switch(t.getType()){case"previousicon":this._previousIconTemplate=t.template;break;case"nexticon":this._nextIconTemplate=t.template;break}})}ngAfterViewInit(){super.ngAfterViewInit(),D(this.platformId)&&this.autoHideButtons&&this.bindResizeObserver()}bindResizeObserver(){this.container=S(this.el.nativeElement,'[data-pc-section="navcontent"]'),this.list=S(this.el.nativeElement,'[data-pc-section="nav"]'),this.resizeObserver=new ResizeObserver(()=>{this.list.offsetWidth>=this.container.offsetWidth?this.buttonVisible=!0:this.buttonVisible=!1,this.updateButtonState(),this.cd.detectChanges()}),this.resizeObserver.observe(this.container)}unbindResizeObserver(){this.resizeObserver.unobserve(this.elementToObserve.nativeElement),this.resizeObserver=null}ngAfterViewChecked(){D(this.platformId)&&this.tabChanged&&(this.updateInkBar(),this.tabChanged=!1)}ngOnDestroy(){this.tabChangesSubscription&&this.tabChangesSubscription.unsubscribe(),this.resizeObserver&&this.unbindResizeObserver(),super.ngOnDestroy()}getTabHeaderActionId(t){return`${t}_header_action`}getTabContentId(t){return`${t}_content`}initTabs(){this.tabs=this.tabPanels.toArray(),!this.findSelectedTab()&&this.tabs.length&&(this.activeIndex!=null&&this.tabs.length>this.activeIndex?this.tabs[this.activeIndex].selected=!0:this.tabs[0].selected=!0,this.tabChanged=!0),this.cd.markForCheck()}onTabKeyDown(t,i){switch(t.code){case"ArrowLeft":this.onTabArrowLeftKey(t);break;case"ArrowRight":this.onTabArrowRightKey(t);break;case"Home":this.onTabHomeKey(t);break;case"End":this.onTabEndKey(t);break;case"PageDown":this.onTabEndKey(t);break;case"PageUp":this.onTabHomeKey(t);break;case"Enter":case"Space":this.open(t,i);break;default:break}}onTabArrowLeftKey(t){let i=this.findPrevHeaderAction(t.currentTarget),n=f(i,"data-pc-index");i?this.changeFocusedTab(t,i,n):this.onTabEndKey(t),t.preventDefault()}onTabArrowRightKey(t){let i=this.findNextHeaderAction(t.currentTarget),n=f(i,"data-pc-index");i?this.changeFocusedTab(t,i,n):this.onTabHomeKey(t),t.preventDefault()}onTabHomeKey(t){let i=this.findFirstHeaderAction(),n=f(i,"data-pc-index");this.changeFocusedTab(t,i,n),t.preventDefault()}onTabEndKey(t){let i=this.findLastHeaderAction(),n=f(i,"data-pc-index");this.changeFocusedTab(t,i,n),t.preventDefault()}changeFocusedTab(t,i,n){if(i&&(ft(i),i.scrollIntoView({block:"nearest"}),this.selectOnFocus)){let a=this.tabs[n];this.open(t,a)}}findNextHeaderAction(t,i=!1){let n=i?t:t.nextElementSibling;return n?f(n,"data-p-disabled")||f(n,"data-pc-section")==="inkbar"?this.findNextHeaderAction(n):n:null}findPrevHeaderAction(t,i=!1){let n=i?t:t.previousElementSibling;return n?f(n,"data-p-disabled")||f(n,"data-pc-section")==="inkbar"?this.findPrevHeaderAction(n):n:null}findFirstHeaderAction(){let t=this.navbar.nativeElement.firstElementChild;return this.findNextHeaderAction(t,!0)}findLastHeaderAction(){let t=this.navbar.nativeElement.lastElementChild,i=f(t,"data-pc-section")==="inkbar"?t.previousElementSibling:t;return this.findPrevHeaderAction(i,!0)}open(t,i){if(i.disabled){t&&t.preventDefault();return}if(!i.selected){let n=this.findSelectedTab();n&&(n.selected=!1),this.tabChanged=!0,i.selected=!0;let a=this.findTabIndex(i);this.preventActiveIndexPropagation=!0,this.activeIndexChange.emit(a),this.onChange.emit({originalEvent:t,index:a}),this.updateScrollBar(a)}t&&t.preventDefault()}close(t,i){this.controlClose?this.onClose.emit({originalEvent:t,index:this.findTabIndex(i),close:()=>{this.closeTab(i)}}):(this.closeTab(i),this.onClose.emit({originalEvent:t,index:this.findTabIndex(i)})),t.stopPropagation()}closeTab(t){if(!t.disabled){if(t.selected){this.tabChanged=!0,t.selected=!1;for(let i=0;i<this.tabs.length;i++){let n=this.tabs[i];if(!n.closed&&!t.disabled){n.selected=!0;break}}}t.closed=!0}}findSelectedTab(){for(let t=0;t<this.tabs.length;t++)if(this.tabs[t].selected)return this.tabs[t];return null}findTabIndex(t){let i=-1;for(let n=0;n<this.tabs.length;n++)if(this.tabs[n]==t){i=n;break}return i}getBlockableElement(){return this.el.nativeElement.children[0]}updateInkBar(){if(D(this.platformId)&&this.navbar){let t=S(this.navbar.nativeElement,'[data-pc-section="headeraction"][data-p-active="true"]');if(!t)return;this.inkbar.nativeElement.style.width=ht(t)+"px",this.inkbar.nativeElement.style.left=j(t).left-j(this.navbar.nativeElement).left+"px"}}updateScrollBar(t){let i=_t(this.navbar.nativeElement,'[data-pc-section="headeraction"]')[t];i&&i.scrollIntoView({block:"nearest"})}updateButtonState(){let t=this.content.nativeElement,{scrollLeft:i,scrollWidth:n}=t,a=k(t);this.backwardIsDisabled=i===0,this.forwardIsDisabled=Math.round(i)===n-a}refreshButtonState(){this.container=S(this.el.nativeElement,'[data-pc-section="navcontent"]'),this.list=S(this.el.nativeElement,'[data-pc-section="nav"]'),this.list.offsetWidth>=this.container.offsetWidth&&(this.list.offsetWidth>=this.container.offsetWidth?this.buttonVisible=!0:this.buttonVisible=!1,this.updateButtonState(),this.cd.markForCheck())}onScroll(t){this.scrollable&&this.updateButtonState(),t.preventDefault()}getVisibleButtonWidths(){return[this.prevBtn?.nativeElement,this.nextBtn?.nativeElement].reduce((t,i)=>i?t+k(i):t,0)}navBackward(){let t=this.content.nativeElement,i=k(t)-this.getVisibleButtonWidths(),n=t.scrollLeft-i;t.scrollLeft=n<=0?0:n}navForward(){let t=this.content.nativeElement,i=k(t)-this.getVisibleButtonWidths(),n=t.scrollLeft+i,a=t.scrollWidth-i;t.scrollLeft=n>=a?a:n}static \u0275fac=(()=>{let t;return function(n){return(t||(t=O(e)))(n||e)}})();static \u0275cmp=B({type:e,selectors:[["p-tabView"],["p-tabview"]],contentQueries:function(i,n,a){if(i&1&&(h(a,Wt,5),h(a,Lt,5),h(a,Et,4),h(a,K,4)),i&2){let s;b(s=d())&&(n.previousIconTemplate=s.first),b(s=d())&&(n.nextIconTemplate=s.first),b(s=d())&&(n.tabPanels=s),b(s=d())&&(n.templates=s)}},viewQuery:function(i,n){if(i&1&&(C(St,5),C(Mt,5),C(zt,5),C(qt,5),C(jt,5),C(Kt,5)),i&2){let a;b(a=d())&&(n.content=a.first),b(a=d())&&(n.navbar=a.first),b(a=d())&&(n.prevBtn=a.first),b(a=d())&&(n.nextBtn=a.first),b(a=d())&&(n.inkbar=a.first),b(a=d())&&(n.elementToObserve=a.first)}},hostVars:11,hostBindings:function(i,n){i&2&&(u("data-pc-name","tabview"),et(n.hostStyle),P(n.hostClass),tt("p-tabs",!0)("p-tabs-scrollable",n.scrollable)("p-component",!0))},inputs:{style:"style",styleClass:"styleClass",controlClose:[2,"controlClose","controlClose",y],scrollable:[2,"scrollable","scrollable",y],activeIndex:"activeIndex",selectOnFocus:[2,"selectOnFocus","selectOnFocus",y],nextButtonAriaLabel:"nextButtonAriaLabel",prevButtonAriaLabel:"prevButtonAriaLabel",autoHideButtons:[2,"autoHideButtons","autoHideButtons",y],tabindex:[2,"tabindex","tabindex",dt]},outputs:{onChange:"onChange",onClose:"onClose",activeIndexChange:"activeIndexChange"},features:[L([F]),N],ngContentSelectors:kt,decls:12,vars:7,consts:[["elementToObserve",""],["content",""],["navbar",""],["prevBtn",""],["inkbar",""],["nextBtn",""],[1,"p-tablist"],["class","p-tablist-prev-button p-tablist-nav-button","type","button","pRipple","",3,"click",4,"ngIf"],[1,"p-tablist-content",3,"scroll","ngClass"],["role","tablist",1,"p-tablist-tab-list"],["class","p-tablist-next-button p-tablist-nav-button","type","button","pRipple","",3,"click",4,"ngIf"],[1,"p-tabpanels"],["type","button","pRipple","",1,"p-tablist-prev-button","p-tablist-nav-button",3,"click"],[4,"ngIf"],[4,"ngTemplateOutlet"],["pRipple","",3,"click","keydown","ngClass","ngStyle","pTooltip","tooltipPosition","positionStyle","tooltipStyleClass","disabled"],["role","presentation",1,"p-tablist-active-bar"],[1,"p-tabview-left-icon",3,"ngClass"],[1,"p-tabview-right-icon",3,"ngClass"],[3,"click"],["type","button","pRipple","",1,"p-tablist-next-button","p-tablist-nav-button",3,"click"]],template:function(i,n){if(i&1){let a=x();Q(),_(0,"div",6,0),l(2,Zt,4,4,"button",7),_(3,"div",8,1),I("scroll",function(At){return v(a),T(n.onScroll(At))}),_(5,"div",9,2),nt(7,me,1,1,null,null,it),m()(),l(9,ye,4,3,"button",10),m(),_(10,"div",11),W(11),m()}i&2&&(p(2),c("ngIf",n.scrollable&&!n.backwardIsDisabled&&n.autoHideButtons),p(),c("ngClass",ct(5,Ut,n.scrollable)),u("data-pc-section","navcontent"),p(2),u("data-pc-section","nav"),p(2),at(n.tabs),p(2),c("ngIf",n.scrollable&&!n.forwardIsDisabled&&n.buttonVisible))},dependencies:[q,pt,M,z,ut,E,It,xt,wt,yt,Tt,Ct],encapsulation:2,changeDetection:0})}return e})(),Ge=(()=>{class e{static \u0275fac=function(i){return new(i||e)};static \u0275mod=Z({type:e});static \u0275inj=X({imports:[$t,Et,E,E]})}return e})();var Ie=3e4,Se=5e3,Vt=5,Ot=class e{constructor(o){this.configService=o}ws=null;reconnectAttempts=0;reconnectTimer=null;heartbeatTimer=null;shouldReconnect=!0;connectionCount=0;connectionState$=new G("DISCONNECTED");message$=new R;error$=new R;connect(){let t=`${this.configService.getWebSocketUrl()}/ws/device/status`;if(this.ws?.readyState===WebSocket.OPEN)return this.connectionCount++,this.connectionState$.asObservable();this.cleanup(),this.shouldReconnect=!0,this.reconnectAttempts=0,this.connectionCount=1,this.connectionState$.next("CONNECTING");try{this.ws=new WebSocket(t),this.ws.onopen=()=>{console.log("\u{1F50C} \u8BBE\u5907\u72B6\u6001 WebSocket \u8FDE\u63A5\u6210\u529F"),this.connectionState$.next("CONNECTED"),this.reconnectAttempts=0,this.startHeartbeat()},this.ws.onmessage=i=>{try{let n=JSON.parse(i.data);console.log("\u{1F4E8} \u8BBE\u5907\u72B6\u6001 WebSocket \u539F\u59CB\u6D88\u606F:",n);let a=n.type?n:{type:"device_status",data:n,timestamp:new Date().toISOString()};this.message$.next(a)}catch(n){console.error("\u89E3\u6790\u8BBE\u5907\u72B6\u6001\u6D88\u606F\u5931\u8D25:",n,"\u539F\u59CB\u6570\u636E:",i.data)}},this.ws.onerror=i=>{console.error("\u8BBE\u5907\u72B6\u6001 WebSocket \u9519\u8BEF:",i),this.connectionState$.next("ERROR"),this.error$.next(i)},this.ws.onclose=i=>{console.log("\u{1F50C} \u8BBE\u5907\u72B6\u6001 WebSocket \u8FDE\u63A5\u5173\u95ED:",i.code,i.reason),this.connectionState$.next("CLOSED"),this.stopHeartbeat(),this.handleReconnect()}}catch(i){console.error("\u8BBE\u5907\u72B6\u6001 WebSocket \u8FDE\u63A5\u5931\u8D25:",i),this.connectionState$.next("ERROR")}return this.connectionState$.asObservable()}disconnect(){console.log("\u{1F50C} \u8BBE\u5907\u72B6\u6001 WebSocket \u65AD\u5F00\u8FDE\u63A5"),this.connectionCount--,this.connectionCount<=0?(this.shouldReconnect=!1,this.cleanup(),this.connectionState$.next("DISCONNECTED"),this.connectionCount=0):console.log(`\u{1F4CA} \u8FD8\u6709 ${this.connectionCount} \u4E2A\u7EC4\u4EF6\u5728\u4F7F\u7528 WebSocket`)}cleanup(){this.stopHeartbeat(),this.clearReconnectTimer(),this.ws&&(this.ws.onopen=null,this.ws.onmessage=null,this.ws.onerror=null,this.ws.onclose=null,this.ws.close(),this.ws=null)}send(o){this.ws?.readyState===WebSocket.OPEN&&this.ws.send(JSON.stringify(o))}getConnectionState(){return this.connectionState$.asObservable()}getMessages(){return this.message$.asObservable()}getErrors(){return this.error$.asObservable()}isConnected(){return this.ws?.readyState===WebSocket.OPEN}handleReconnect(){if(this.shouldReconnect){if(this.reconnectAttempts>=Vt){console.error("\u8BBE\u5907\u72B6\u6001 WebSocket \u91CD\u8FDE\u6B21\u6570\u8D85\u8FC7\u9650\u5236");return}this.reconnectTimer=setTimeout(()=>{this.reconnectAttempts++,console.log(`\u{1F504} \u8BBE\u5907\u72B6\u6001 WebSocket \u5C1D\u8BD5\u91CD\u8FDE (${this.reconnectAttempts}/${Vt})`),this.connect()},Se)}}clearReconnectTimer(){this.reconnectTimer&&(clearTimeout(this.reconnectTimer),this.reconnectTimer=null)}startHeartbeat(){this.stopHeartbeat(),this.heartbeatTimer=setInterval(()=>{this.ws?.readyState===WebSocket.OPEN&&this.send({type:"ping"})},Ie)}stopHeartbeat(){this.heartbeatTimer&&(clearInterval(this.heartbeatTimer),this.heartbeatTimer=null)}ngOnDestroy(){this.disconnect()}static \u0275fac=function(t){return new(t||e)(Y(vt))};static \u0275prov=$({token:e,factory:e.\u0275fac,providedIn:"root"})};export{Et as a,$t as b,Ge as c,Ot as d};
