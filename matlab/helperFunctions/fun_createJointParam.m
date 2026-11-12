function jointStruct = fun_createJointParam(jointStruct)
   % fun_createJointParam updates joint parameters within a joint structure
    %
    % This function iterates through each joint in the input jointStruct
    % and calculates various joint parameters, including constraint Jacobian matrices (Gi, Gj, Hj),
    % transformation matrices (Tvi, Tvj, Tfi, Tfj), and offset parameters (rji, Bji).
    % These parameters are based on the joint's type, end-effector basis, screw axis, and body center of mass.
    % The function handles primary joints and updates the jointStruct with the calculated parameters.
    %
    % Inputs:
    %   jointStruct - A structure array containing parameters of each joint
    %
    % Outputs:
    %   jointStruct - The updated structure array with new joint parameters
    %   input   -> eBasis,si,sj,ipoint,jpoint,icm,jcm,i_reloDriveConstrCols,j_reloDriveConstrCols 
    %   output  -> Gi,Gj,Hj,Ti,Tj,rji,Bji

    numJoints = length(jointStruct);

    for i = 1:numJoints
        [Gi,Gj,Hj] = createJointConstJakMat(...
        jointStruct(i).i.eBasis,jointStruct(i).j.eBasis,...
        jointStruct(i).i.s,jointStruct(i).j.s,...
        jointStruct(i).i.point,jointStruct(i).j.point,...
        jointStruct(i).type);
        %
        jointStruct(i).Gi=Gi;
        jointStruct(i).Gj=Gj;
        jointStruct(i).Hj=Hj;
        %
        [jointStruct(i).Ti,jointStruct(i).Ti_inv] = createJointTransMat(createSkewSymMat(jointStruct(i).i.s));
        [jointStruct(i).Tj,jointStruct(i).Tj_inv] = createJointTransMat(createSkewSymMat(jointStruct(i).j.s));
        %
        if jointStruct(i).i.bodyID ==0 || jointStruct(i).j.bodyID ==0  
            jointStruct(i).rji = NaN(3,1);
            jointStruct(i).Bji =  NaN(6);

        elseif jointStruct(i).i.bodyID > jointStruct(i).j.bodyID
            [rji,Bji] = createOffsetParam (jointStruct(i).i.bodyCM,jointStruct(i).j.bodyCM);
            jointStruct(i).rji =  rji;
            jointStruct(i).Bji =  Bji;
        else
            [rji,Bji] = createOffsetParam (jointStruct(i).j.bodyCM,jointStruct(i).i.bodyCM);
            jointStruct(i).rji =  rji;
            jointStruct(i).Bji =  Bji;
        end
        %
        %
        if isfield(jointStruct(i).i, 'reloDriveConstrCols')
            jointStruct(i).Gi  = [jointStruct(i).Gi;jointStruct(i).i.reloDriveConstrCols];
            jointStruct(i).Gj  = [jointStruct(i).Gj;jointStruct(i).j.reloDriveConstrCols];
        end

    end
end

