import{Ga as O,Ha as _,Ka as Z,Pa as q,Qa as f,c as Q,e as R,g as V,j as G}from"./chunk-IB6J4LGI.js";import{$ as D,Ab as r,Bb as p,Cb as B,Db as L,Kb as v,Lb as S,Mb as j,Pb as b,Rb as I,Sb as T,Ta as d,V as $,W as F,Wb as E,Xb as N,cc as z,db as s,eb as k,ha as g,hb as c,ja as i,jb as y,pb as u,qb as m,tb as x,ub as l,wc as H,zb as o}from"./chunk-IJGG6W7R.js";var P=["icon"],J=["*"];function K(e,C){if(e&1&&p(0,"span",4),e&2){let t=v(2);m("ngClass",t.icon)}}function U(e,C){if(e&1&&(B(0),y(1,K,1,1,"span",3),L()),e&2){let t=v();d(),m("ngIf",t.icon)}}function W(e,C){}function X(e,C){e&1&&y(0,W,0,0,"ng-template")}function Y(e,C){if(e&1&&(o(0,"span",5),y(1,X,1,0,null,6),r()),e&2){let t=v();d(),m("ngTemplateOutlet",t.iconTemplate||t._iconTemplate)}}var ee=({dt:e})=>`
.p-tag {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: ${e("tag.primary.background")};
    color: ${e("tag.primary.color")};
    font-size: ${e("tag.font.size")};
    font-weight: ${e("tag.font.weight")};
    padding: ${e("tag.padding")};
    border-radius: ${e("tag.border.radius")};
    gap: ${e("tag.gap")};
}

.p-tag-icon {
    font-size: ${e("tag.icon.size")};
    width: ${e("tag.icon.size")};
    height:${e("tag.icon.size")};
}

.p-tag-rounded {
    border-radius: ${e("tag.rounded.border.radius")};
}

.p-tag-success {
    background: ${e("tag.success.background")};
    color: ${e("tag.success.color")};
}

.p-tag-info {
    background: ${e("tag.info.background")};
    color: ${e("tag.info.color")};
}

.p-tag-warn {
    background: ${e("tag.warn.background")};
    color: ${e("tag.warn.color")};
}

.p-tag-danger {
    background: ${e("tag.danger.background")};
    color: ${e("tag.danger.color")};
}

.p-tag-secondary {
    background: ${e("tag.secondary.background")};
    color: ${e("tag.secondary.color")};
}

.p-tag-contrast {
    background: ${e("tag.contrast.background")};
    color: ${e("tag.contrast.color")};
}
`,te={root:({props:e})=>["p-tag p-component",{"p-tag-info":e.severity==="info","p-tag-success":e.severity==="success","p-tag-warn":e.severity==="warn","p-tag-danger":e.severity==="danger","p-tag-secondary":e.severity==="secondary","p-tag-contrast":e.severity==="contrast","p-tag-rounded":e.rounded}],icon:"p-tag-icon",label:"p-tag-label"},A=(()=>{class e extends Z{name="tag";theme=ee;classes=te;static \u0275fac=(()=>{let t;return function(n){return(t||(t=i(e)))(n||e)}})();static \u0275prov=$({token:e,factory:e.\u0275fac})}return e})();var ne=(()=>{class e extends q{get style(){return this._style}set style(t){this._style=t,this.cd.markForCheck()}styleClass;severity;value;icon;rounded;iconTemplate;templates;_iconTemplate;_style;_componentStyle=D(A);ngAfterContentInit(){this.templates?.forEach(t=>{switch(t.getType()){case"icon":this._iconTemplate=t.template;break}})}containerClass(){let t="p-tag p-component";return this.severity&&(t+=` p-tag-${this.severity}`),this.rounded&&(t+=" p-tag-rounded"),this.styleClass&&(t+=` ${this.styleClass}`),t}static \u0275fac=(()=>{let t;return function(n){return(t||(t=i(e)))(n||e)}})();static \u0275cmp=s({type:e,selectors:[["p-tag"]],contentQueries:function(a,n,M){if(a&1&&(b(M,P,4),b(M,O,4)),a&2){let h;I(h=T())&&(n.iconTemplate=h.first),I(h=T())&&(n.templates=h)}},hostVars:4,hostBindings:function(a,n){a&2&&(x(n.style),l(n.containerClass()))},inputs:{style:"style",styleClass:"styleClass",severity:"severity",value:"value",icon:"icon",rounded:[2,"rounded","rounded",H]},features:[z([A]),c],ngContentSelectors:J,decls:5,vars:3,consts:[[4,"ngIf"],["class","p-tag-icon",4,"ngIf"],[1,"p-tag-label"],["class","p-tag-icon",3,"ngClass",4,"ngIf"],[1,"p-tag-icon",3,"ngClass"],[1,"p-tag-icon"],[4,"ngTemplateOutlet"]],template:function(a,n){a&1&&(S(),j(0),y(1,U,2,1,"ng-container",0)(2,Y,2,1,"span",1),o(3,"span",2),E(4),r()),a&2&&(d(),m("ngIf",!n.iconTemplate&&!n._iconTemplate),d(),m("ngIf",n.iconTemplate||n._iconTemplate),d(2),N(n.value))},dependencies:[G,Q,R,V,_],encapsulation:2,changeDetection:0})}return e})(),Ce=(()=>{class e{static \u0275fac=function(a){return new(a||e)};static \u0275mod=k({type:e});static \u0275inj=F({imports:[ne,_,_]})}return e})();var _e=(()=>{class e extends f{static \u0275fac=(()=>{let t;return function(n){return(t||(t=i(e)))(n||e)}})();static \u0275cmp=s({type:e,selectors:[["ChevronLeftIcon"]],features:[c],decls:2,vars:5,consts:[["width","14","height","14","viewBox","0 0 14 14","fill","none","xmlns","http://www.w3.org/2000/svg"],["d","M9.61296 13C9.50997 13.0005 9.40792 12.9804 9.3128 12.9409C9.21767 12.9014 9.13139 12.8433 9.05902 12.7701L3.83313 7.54416C3.68634 7.39718 3.60388 7.19795 3.60388 6.99022C3.60388 6.78249 3.68634 6.58325 3.83313 6.43628L9.05902 1.21039C9.20762 1.07192 9.40416 0.996539 9.60724 1.00012C9.81032 1.00371 10.0041 1.08597 10.1477 1.22959C10.2913 1.37322 10.3736 1.56698 10.3772 1.77005C10.3808 1.97313 10.3054 2.16968 10.1669 2.31827L5.49496 6.99022L10.1669 11.6622C10.3137 11.8091 10.3962 12.0084 10.3962 12.2161C10.3962 12.4238 10.3137 12.6231 10.1669 12.7701C10.0945 12.8433 10.0083 12.9014 9.91313 12.9409C9.81801 12.9804 9.71596 13.0005 9.61296 13Z","fill","currentColor"]],template:function(a,n){a&1&&(g(),o(0,"svg",0),p(1,"path",1),r()),a&2&&(l(n.getClassNames()),u("aria-label",n.ariaLabel)("aria-hidden",n.ariaHidden)("role",n.role))},encapsulation:2})}return e})();var Te=(()=>{class e extends f{static \u0275fac=(()=>{let t;return function(n){return(t||(t=i(e)))(n||e)}})();static \u0275cmp=s({type:e,selectors:[["ChevronRightIcon"]],features:[c],decls:2,vars:5,consts:[["width","14","height","14","viewBox","0 0 14 14","fill","none","xmlns","http://www.w3.org/2000/svg"],["d","M4.38708 13C4.28408 13.0005 4.18203 12.9804 4.08691 12.9409C3.99178 12.9014 3.9055 12.8433 3.83313 12.7701C3.68634 12.6231 3.60388 12.4238 3.60388 12.2161C3.60388 12.0084 3.68634 11.8091 3.83313 11.6622L8.50507 6.99022L3.83313 2.31827C3.69467 2.16968 3.61928 1.97313 3.62287 1.77005C3.62645 1.56698 3.70872 1.37322 3.85234 1.22959C3.99596 1.08597 4.18972 1.00371 4.3928 1.00012C4.59588 0.996539 4.79242 1.07192 4.94102 1.21039L10.1669 6.43628C10.3137 6.58325 10.3962 6.78249 10.3962 6.99022C10.3962 7.19795 10.3137 7.39718 10.1669 7.54416L4.94102 12.7701C4.86865 12.8433 4.78237 12.9014 4.68724 12.9409C4.59212 12.9804 4.49007 13.0005 4.38708 13Z","fill","currentColor"]],template:function(a,n){a&1&&(g(),o(0,"svg",0),p(1,"path",1),r()),a&2&&(l(n.getClassNames()),u("aria-label",n.ariaLabel)("aria-hidden",n.ariaHidden)("role",n.role))},encapsulation:2})}return e})();var $e=(()=>{class e extends f{static \u0275fac=(()=>{let t;return function(n){return(t||(t=i(e)))(n||e)}})();static \u0275cmp=s({type:e,selectors:[["MinusIcon"]],features:[c],decls:2,vars:5,consts:[["width","14","height","14","viewBox","0 0 14 14","fill","none","xmlns","http://www.w3.org/2000/svg"],["d","M13.2222 7.77778H0.777778C0.571498 7.77778 0.373667 7.69584 0.227806 7.54998C0.0819442 7.40412 0 7.20629 0 7.00001C0 6.79373 0.0819442 6.5959 0.227806 6.45003C0.373667 6.30417 0.571498 6.22223 0.777778 6.22223H13.2222C13.4285 6.22223 13.6263 6.30417 13.7722 6.45003C13.9181 6.5959 14 6.79373 14 7.00001C14 7.20629 13.9181 7.40412 13.7722 7.54998C13.6263 7.69584 13.4285 7.77778 13.2222 7.77778Z","fill","currentColor"]],template:function(a,n){a&1&&(g(),o(0,"svg",0),p(1,"path",1),r()),a&2&&(l(n.getClassNames()),u("aria-label",n.ariaLabel)("aria-hidden",n.ariaHidden)("role",n.role))},encapsulation:2})}return e})();export{_e as a,Te as b,$e as c,ne as d,Ce as e};
