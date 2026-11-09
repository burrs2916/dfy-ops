import{Ha as i,Ka as C,Pa as F,c as D,f as S,j as M}from"./chunk-BEYVZD3X.js";import{$ as d,Ab as k,Bb as v,Ta as a,V as l,W as c,cc as b,db as f,eb as m,ha as g,hb as u,ja as o,pb as t,qb as y,rb as h,zb as p}from"./chunk-IJGG6W7R.js";var I=({dt:e})=>`
.p-progressspinner {
    position: relative;
    margin: 0 auto;
    width: 100px;
    height: 100px;
    display: inline-block;
}

.p-progressspinner::before {
    content: "";
    display: block;
    padding-top: 100%;
}

.p-progressspinner-spin {
    height: 100%;
    transform-origin: center center;
    width: 100%;
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    right: 0;
    margin: auto;
    animation: p-progressspinner-rotate 2s linear infinite;
}

.p-progressspinner-circle {
    stroke-dasharray: 89, 200;
    stroke-dashoffset: 0;
    stroke: ${e("progressspinner.colorOne")};
    animation: p-progressspinner-dash 1.5s ease-in-out infinite, p-progressspinner-color 6s ease-in-out infinite;
    stroke-linecap: round;
}

@keyframes p-progressspinner-rotate {
    100% {
        transform: rotate(360deg);
    }
}
@keyframes p-progressspinner-dash {
    0% {
        stroke-dasharray: 1, 200;
        stroke-dashoffset: 0;
    }
    50% {
        stroke-dasharray: 89, 200;
        stroke-dashoffset: -35px;
    }
    100% {
        stroke-dasharray: 89, 200;
        stroke-dashoffset: -124px;
    }
}
@keyframes p-progressspinner-color {
    100%,
    0% {
        stroke: ${e("progressspinner.colorOne")};
    }
    40% {
        stroke: ${e("progressspinner.colorTwo")};
    }
    66% {
        stroke: ${e("progressspinner.colorThree")};
    }
    80%,
    90% {
        stroke: ${e("progressspinner.colorFour")};
    }
}
`,j={root:"p-progressspinner",spin:"p-progressspinner-spin",circle:"p-progressspinner-circle"},w=(()=>{class e extends C{name="progressspinner";theme=I;classes=j;static \u0275fac=(()=>{let s;return function(r){return(s||(s=o(e)))(r||e)}})();static \u0275prov=l({token:e,factory:e.\u0275fac})}return e})();var B=(()=>{class e extends F{styleClass;style;strokeWidth="2";fill="none";animationDuration="2s";ariaLabel;_componentStyle=d(w);static \u0275fac=(()=>{let s;return function(r){return(s||(s=o(e)))(r||e)}})();static \u0275cmp=f({type:e,selectors:[["p-progressSpinner"],["p-progress-spinner"],["p-progressspinner"]],inputs:{styleClass:"styleClass",style:"style",strokeWidth:"strokeWidth",fill:"fill",animationDuration:"animationDuration",ariaLabel:"ariaLabel"},features:[b([w]),u],decls:3,vars:11,consts:[["role","progressbar",1,"p-progressspinner",3,"ngStyle","ngClass"],["viewBox","25 25 50 50",1,"p-progressspinner-spin"],["cx","50","cy","50","r","20","stroke-miterlimit","10",1,"p-progressspinner-circle"]],template:function(n,r){n&1&&(p(0,"div",0),g(),p(1,"svg",1),v(2,"circle",2),k()()),n&2&&(y("ngStyle",r.style)("ngClass",r.styleClass),t("aria-label",r.ariaLabel)("aria-busy",!0)("data-pc-name","progressspinner")("data-pc-section","root"),a(),h("animation-duration",r.animationDuration),t("data-pc-section","root"),a(),t("fill",r.fill)("stroke-width",r.strokeWidth))},dependencies:[M,D,S,i],encapsulation:2,changeDetection:0})}return e})(),A=(()=>{class e{static \u0275fac=function(n){return new(n||e)};static \u0275mod=m({type:e});static \u0275inj=c({imports:[B,i,i]})}return e})();export{B as a,A as b};
