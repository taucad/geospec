# Writes the faceted L-plate with a square through-hole: plate-good.step, and with the
# hole's FACE_BOUND flag reversed on the top face (plate-top.step) or on both faces
# (plate-both.step). From the S2 attempt-1 review. With an argument, the hole's side
# in mm (default 2), and the files are named plate-small-*.step: the attempt-2 review's
# hole of 0.3 mm, whose edges are all shorter than the orientation probe's reach at a
# 1e-2 mm floor.
import sys
HOLE=float(sys.argv[1]) if len(sys.argv)>1 else 2.0
NAME='plate-small' if len(sys.argv)>1 else 'plate'
def gen(top_inner, bot_inner):
    L=[(0,0),(20,0),(20,3),(10,3),(10,10),(0,10)]
    H=[(4,4),(4+HOLE,4),(4+HOLE,4+HOLE),(4,4+HOLE)]
    pts={}
    for i,(x,y) in enumerate(L): pts[100+i]=(x,y,0); pts[120+i]=(x,y,2)
    for i,(x,y) in enumerate(H): pts[110+i]=(x,y,0); pts[130+i]=(x,y,2)
    faces=[]  # list of list of (loop, outer?, flag)
    faces.append([([100,105,104,103,102,101],True,'.T.'),([110,111,112,113],False,bot_inner)])
    faces.append([([120,121,122,123,124,125],True,'.T.'),([130,133,132,131],False,top_inner)])
    n=len(L)
    for i in range(n):
        j=(i+1)%n
        faces.append([([100+i,100+j,120+j,120+i],True,'.T.')])
    for a,b,c,d in [(110,130,131,111),(111,131,132,112),(112,132,133,113),(113,133,130,110)]:
        faces.append([([a,b,c,d],True,'.T.')])
    out=["ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;DATA;"]
    eid=1000; fids=[]
    for f in faces:
        bids=[]
        for loop,outer,flag in f:
            lid=eid; eid+=1
            out.append(f"#{lid}=POLY_LOOP('',({','.join('#%d'%p for p in loop)}));")
            bid=eid; eid+=1
            out.append(f"#{bid}={'FACE_OUTER_BOUND' if outer else 'FACE_BOUND'}('',#{lid},{flag});")
            bids.append(bid)
        fid=eid; eid+=1
        out.append(f"#{fid}=FACE('',({','.join('#%d'%b for b in bids)}));"); fids.append(fid)
    out.append(f"#2=CLOSED_SHELL('',({','.join('#%d'%f for f in fids)}));")
    out.append("#1=FACETED_BREP('',#2);")
    num=lambda v: f"{int(v)}." if float(v).is_integer() else repr(float(v)).upper()
    for k,(x,y,z) in pts.items(): out.append(f"#{k}=CARTESIAN_POINT('',({num(x)},{num(y)},{num(z)}));")
    out.append("#200=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#201=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#202=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#200,#201)) REPRESENTATION_CONTEXT('',''));#203=FACETED_BREP_SHAPE_REPRESENTATION('',(#1),#202);ENDSEC;END-ISO-10303-21;")
    return "\n".join(out)
open(f'{NAME}-good.step','w').write(gen('.T.','.T.'))
open(f'{NAME}-both.step','w').write(gen('.F.','.F.'))
open(f'{NAME}-top.step','w').write(gen('.F.','.T.'))
