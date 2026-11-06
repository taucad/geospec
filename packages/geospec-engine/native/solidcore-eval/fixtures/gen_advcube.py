# Writes a 10 mm cube of ADVANCED_FACEs (advcube.step), the same with the top face's
# same_sense reversed (advcube-flip.step), and a 1 mm cube whose top plane is tilted
# by the given slope (advcube-tilt.step, default 4e-6), and by 1e-6
# (advcube-tilt-small.step: its trims sit within 2e-6 mm of their edges, inside their
# own gap). Run: python3 gen_advcube.py 4e-6
import sys
# 10 mm cube of ADVANCED_FACEs on planes; `flip` reverses one face's same_sense.
def gen(flip=None, a=10, tilt=0.0):
    V=[(x,y,z) for z in (0,a) for y in (0,a) for x in (0,a)]
    idx=lambda x,y,z: V.index((x,y,z))
    # faces: (normal axis, sign, ccw vertex loop seen from outside)
    F=[((0,0,-1),[(0,0,0),(0,a,0),(a,a,0),(a,0,0)]),
       ((tilt,0,1),[(0,0,a),(a,0,a),(a,a,a),(0,a,a)]),
       ((0,-1,0),[(0,0,0),(a,0,0),(a,0,a),(0,0,a)]),
       ((0,1,0),[(0,a,0),(0,a,a),(a,a,a),(a,a,0)]),
       ((-1,0,0),[(0,0,0),(0,0,a),(0,a,a),(0,a,0)]),
       ((1,0,0),[(a,0,0),(a,a,0),(a,a,a),(a,0,a)])]
    out=[]; n=[1000]
    def e(s):
        i=n[0]; n[0]+=1; out.append(f"#{i}={s};"); return i
    f3=lambda p: "(%s)"%",".join(repr(float(c)).upper() for c in p)
    pts=[e(f"CARTESIAN_POINT('',{f3(p)})") for p in V]
    vps=[e(f"VERTEX_POINT('',#{p})") for p in pts]
    edges={}
    def edge(a,b):
        key=(min(a,b),max(a,b))
        if key not in edges:
            i,j=key; pa,pb=V[i],V[j]
            d=tuple(pb[k]-pa[k] for k in range(3)); L=max(abs(c) for c in d); u=tuple(c/L for c in d)
            dr=e(f"DIRECTION('',{f3(u)})"); vec=e(f"VECTOR('',#{dr},{float(L)!r})")
            ln=e(f"LINE('',#{pts[i]},#{vec})")
            edges[key]=e(f"EDGE_CURVE('',#{vps[i]},#{vps[j]},#{ln},.T.)")
        return edges[key], (a,b)==key
    faces=[]
    for fi,(nrm,loop) in enumerate(F):
        ids=[idx(*p) for p in loop]
        oes=[]
        for k in range(4):
            ec,fw=edge(ids[k],ids[(k+1)%4])
            oes.append(e(f"ORIENTED_EDGE('',*,*,#{ec},{'.T.' if fw else '.F.'})"))
        el=e(f"EDGE_LOOP('',({','.join('#%d'%o for o in oes)}))")
        fb=e(f"FACE_OUTER_BOUND('',#{el},.T.)")
        org=e(f"CARTESIAN_POINT('',{f3(loop[0])})"); ax=e(f"DIRECTION('',{f3(nrm)})")
        ref=tuple(1.0 if (abs(nrm[0])<0.5 and k==1 and nrm[1]==0) or (abs(nrm[0])<0.5 and nrm[1]!=0 and k==0) or (abs(nrm[0])>=0.5 and k==1) else 0.0 for k in range(3))
        rd=e(f"DIRECTION('',{f3(ref)})")
        pl=e(f"PLANE('',#{e(f'AXIS2_PLACEMENT_3D({chr(39)}{chr(39)},#{org},#{ax},#{rd})')})")
        ss='.F.' if flip==fi else '.T.'
        faces.append(e(f"ADVANCED_FACE('',(#{fb}),#{pl},{ss})"))
    sh=e(f"CLOSED_SHELL('',({','.join('#%d'%f for f in faces)}))")
    br=e(f"MANIFOLD_SOLID_BREP('',#{sh})")
    out.append(f"#200=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#201=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#202=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#200,#201)) REPRESENTATION_CONTEXT('',''));#203=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{br}),#202);")
    return "ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;DATA;"+"".join(out)+"ENDSEC;END-ISO-10303-21;"
open('advcube.step','w').write(gen())
open('advcube-flip.step','w').write(gen(1))
open('advcube-tilt.step','w').write(gen(None, 1, float(sys.argv[1]) if len(sys.argv)>1 else 4e-6))
open('advcube-tilt-small.step','w').write(gen(None, 1, 1e-6))
