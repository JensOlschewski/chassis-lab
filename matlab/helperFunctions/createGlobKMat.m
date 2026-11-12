function K = createGlobKMat(bushStruct, uMode, varargin)
    % ##################################################################################
    % Function: createGlobKMat
    % Description: Creates the global coupled stiffness matrix
    % This function constructs the global coupled stiffness matrix K from the
    % coupled stiffness matrices in bushStruct. The matrix K has the size
    % (6*nB, 6*nB), where nB is the number of bodies.
    %
    % Inputs:
    %   bushStruct - Structure containing coupled stiffness matrices and bodyIDs
    %   uMode      - Mode of the global matrix (absolute or relative)
    %   nB varargin{1} - Number of bodies
    %   nJ varargin{2} - Number of joints
    %   
    %
    % Output:
    %   K          - The global coupled stiffness matrix
    %
    % The function processes each active element in bushStruct. It adds the
    % relevant parts of the coupled matrix to K based on the bodyID values. If either
    % ibodyID or jbodyID is zero, only the corresponding non-zero blocks of the
    % coupled matrix are added to K.
    %
    % Add relative mode 
    % uMode = relative 
    % uMode = global
    % ##################################################################################

    switch uMode

        case 'absolute' 
            nB = varargin{1};
            K = zeros(6*nB, 6*nB);
            KElem = K;
        
        case 'relative'
            nB = varargin{1};
            if isempty(varargin{2})
                error('nJ must be specified when uMode is ''relative''.');
            end
            nJ = varargin{2};
            K = zeros(6*nB, 6*nJ);
            KElem = K;
    
    end

    % Iterate through all elements in bushStruct
    for i = 1:length(bushStruct)
        if bushStruct(i).active
            % Extract the coupled stiffness matrix and bodyIDs
            kMatrixCoup = bushStruct(i).charStruct.kMatrixCoup;
            ibodyID = bushStruct(i).i.bodyID;
            jbodyID = bushStruct(i).j.bodyID;

    switch uMode 

        case 'absolute'
            if ibodyID ~= 0 
                iRowIndices = 6*ibodyID-5 : 6*ibodyID;
                KElem(iRowIndices, iRowIndices) = kMatrixCoup(1:6, 1:6);
            end
            if jbodyID ~= 0 
                jColumnIndices = 6*jbodyID-5 : 6*jbodyID;
                KElem(jColumnIndices, jColumnIndices) = kMatrixCoup(7:12, 7:12);
            end

            if ibodyID ~= 0 && jbodyID ~= 0 && strcmp(uMode, 'absolute') 
                KElem(iRowIndices, jColumnIndices) = kMatrixCoup(1:6, 7:12);
                KElem(jColumnIndices, iRowIndices) = kMatrixCoup(7:12, 1:6);
            end

        case 'relative'
            if ibodyID ~= 0 
                iRowIndices = 6*ibodyID-5 : 6*ibodyID;
                iColIndices = 6*i - 5: 6*i;
                KElem(iRowIndices, iColIndices) = kMatrixCoup(1:6, 1:6);
            end
            if jbodyID ~= 0 
                jRowIndices = 6*jbodyID-5 : 6*jbodyID;
                jColIndices = 6*i - 5: 6*i;
                KElem(jRowIndices, jColIndices) = kMatrixCoup(7:12, 7:12);
            end
    end
        
            
            % Add blocks of the coupled matrix to KElem based on bodyIDs

            % Update global matrix K and reset KElem
            K = K + KElem;
            KElem(:) = 0;
        end
    end
end
